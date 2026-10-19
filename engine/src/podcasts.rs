//! Podcasts from three places at once, none needing an account of its own:
//! Pandora's catalog, Apple's public directory (which leads to a show's RSS
//! feed), and any feed added by its address. The same show found in several
//! is listed once, from whichever has its newest episode.
//!
//! A show is known by its key: a Pandora id (`PC:…`) or its feed's address.
//! Subscriptions and how far into each episode you are stay here, in
//! Omadora's data, and episodes stream from wherever they're published.

use crate::discover::{public_host, read_capped, shorten, strip_html, web_url};
use crate::pandora::{Pandora, USER_AGENT};
use crate::stream::Download;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Long-running shows have big feeds.
const FEED_MAX_BYTES: u64 = 40 << 20;
const FEED_KEEP: Duration = Duration::from_secs(30 * 60);
const TRENDING_KEEP: Duration = Duration::from_secs(60 * 60);
const DIRECTORY_MAX_BYTES: u64 = 8 << 20;
// Episodes kept on disk, newest played first, up to this much.
const CACHE_MAX: u64 = 2 << 30;
// A show's page lists this many of its newest episodes.
const PAGE_EPISODES: usize = 300;
const SUMMARY_LENGTH: usize = 400;
// Within this many seconds of the end counts as played.
const END_SLACK: f64 = 30.0;
// Audio to have in hand before an episode starts.
const START_BYTES: u64 = 256 << 10;
const ART_HOST: &str = "https://content-images.p-cdn.com/";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Subscriptions and progress: in each account's own folder.
fn library_path(user: &str) -> PathBuf {
    crate::files::account_dir_for(user).join("podcasts.json")
}

fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".cache"));
    base.join("omadora").join("episodes")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn hex_id(text: &str, len: usize) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..len].to_string()
}

/// Redirects only to other public web addresses; podcast hosts chain a few
/// (analytics prefixes) before the file.
fn redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let next = attempt.url().as_str();
        if attempt.previous().len() >= 10 || !web_url(next) || !public_host(next) {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

static HTTP: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(format!("{USER_AGENT} podcast player (+https://github.com/scttymn/omadora)"))
        .timeout(Duration::from_secs(30))
        .redirect(redirects())
        .build()
        .unwrap_or_default()
});

// Episodes take as long as they take; only connecting is timed.
static AUDIO_HTTP: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(format!("{USER_AGENT} podcast player (+https://github.com/scttymn/omadora)"))
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .redirect(redirects())
        .build()
        .unwrap_or_default()
});

fn get_json(request: reqwest::blocking::RequestBuilder) -> Result<Value> {
    let response = request.send().map_err(reqwest::Error::without_url)?.error_for_status().map_err(reqwest::Error::without_url)?;
    Ok(serde_json::from_slice(&read_capped(response, DIRECTORY_MAX_BYTES)?)?)
}

/// Seconds since 1970 from an ISO 8601 time such as Pandora's
/// "2026-10-13T02:00:00.000-07:00" or Apple's "2026-10-13T09:00:00Z".
fn iso_time(text: &str) -> i64 {
    let num = |s: &str| s.parse::<i64>().ok();
    let (Some(y), Some(mo), Some(d)) = (text.get(0..4).and_then(num), text.get(5..7).and_then(num), text.get(8..10).and_then(num)) else {
        return 0;
    };
    let (h, mi, se) = (text.get(11..13).and_then(num).unwrap_or(0), text.get(14..16).and_then(num).unwrap_or(0), text.get(17..19).and_then(num).unwrap_or(0));
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut seconds = days * 86400 + h * 3600 + mi * 60 + se;
    // The offset after the time, if any: local time minus it is UTC.
    let zone = &text[19.min(text.len())..];
    if let Some(at) = zone.rfind(['+', '-']) {
        let sign = if &zone[at..at + 1] == "-" { -1 } else { 1 };
        let (oh, om) = (zone.get(at + 1..at + 3).and_then(num).unwrap_or(0), zone.get(at + 4..at + 6).and_then(num).unwrap_or(0));
        seconds -= sign * (oh * 3600 + om * 60);
    }
    seconds
}

/// Titles compared loosely, to spot one show in several places.
fn same_show(title: &str) -> String {
    title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

// ---- Pandora's podcasts

static LISTENER: Mutex<Option<(Option<String>, String)>> = Mutex::new(None);

/// Pandora's podcast calls want the listener's id, which the collection
/// call says. Kept per account.
fn listener_id(pandora: &Pandora) -> Result<String> {
    let user = crate::pandora::saved_user();
    if let Some((owner, id)) = LISTENER.lock().unwrap().as_ref()
        && *owner == user
    {
        return Ok(id.clone());
    }
    let found = pandora.call("v4/collections/getItems", json!({ "limit": 1 }))?;
    let id = match &found["listenerId"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("Pandora didn't say who's listening"),
    };
    *LISTENER.lock().unwrap() = Some((user, id.clone()));
    Ok(id)
}

fn aesop(pandora: &Pandora, path: &str, mut body: Value) -> Result<Value> {
    body["listenerId"] = json!(listener_id(pandora)?);
    body["catalogVersion"] = json!(4);
    pandora.call(path, body)
}

fn pandora_art(icon: &Value) -> String {
    match icon["artId"].as_str() {
        Some(id) if !id.is_empty() && !id.contains("..") => format!("{ART_HOST}{id}_500W_500H.jpg"),
        _ => String::new(),
    }
}

/// Details for Pandora podcast ids, a hundred at a time.
fn pandora_annotate(pandora: &Pandora, ids: &[String]) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(100) {
        if let Ok(found) = aesop(pandora, "v1/aesop/annotateObjects", json!({ "pandoraIds": chunk, "withProgress": false })) {
            let map = found.get("annotations").unwrap_or(&found);
            if let Some(map) = map.as_object() {
                out.extend(map.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
    }
    out
}

fn pandora_episode(a: &Value, show: &str, show_title: &str, show_art: &str) -> Value {
    let art = pandora_art(&a["icon"]);
    let program = a["programName"].as_str().filter(|n| !n.is_empty()).unwrap_or(show_title);
    json!({
        "id": a["pandoraId"],
        "kind": "episode",
        "source": "pandora",
        "title": a["name"].as_str().unwrap_or_default(),
        "artist": program,
        "album": "",
        "show": show,
        "art": if art.is_empty() { show_art.to_string() } else { art },
        "duration": a["duration"].as_u64(),
        "published": iso_time(a["releaseDate"].as_str().unwrap_or_default()),
        "summary": shorten(&strip_html(a["summary"].as_str().unwrap_or_default()), SUMMARY_LENGTH),
        "playable": true,
        "encoding": "mp3",
    })
}

/// A Pandora show and its newest episodes.
fn pandora_show(pandora: &Pandora, id: &str) -> Result<Show> {
    // Pandora answers nothing at all past 20 annotated; the rest are
    // annotated after.
    let found = aesop(pandora, "v1/aesop/getAllEpisodesByPodcastProgram", json!({ "pandoraId": id, "annotationLimit": 20, "sortingOrder": null }))?;
    let mut annotations: HashMap<String, Value> =
        found["episodes"]["annotations"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    let ids: Vec<String> = found["episodes"]["episodesWithLabel"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["episodes"].as_array().cloned().unwrap_or_default())
        .filter_map(|e| e.as_str().map(str::to_string))
        .take(PAGE_EPISODES)
        .collect();
    let missing: Vec<String> = ids.iter().filter(|e| !annotations.contains_key(*e)).cloned().collect();
    annotations.extend(pandora_annotate(pandora, &missing));
    let program = annotations.get(id).cloned().unwrap_or(Value::Null);
    let title = program["name"].as_str().unwrap_or_default().to_string();
    let art = pandora_art(&program["icon"]);
    let mut episodes: Vec<Value> = ids.iter().filter_map(|e| annotations.get(e)).filter(|a| a["type"] == "PE").map(|a| pandora_episode(a, id, &title, &art)).collect();
    episodes.sort_by_key(|e| std::cmp::Reverse(e["published"].as_i64().unwrap_or(0)));
    let link = program["shareableUrlPath"].as_str().map(|p| format!("https://www.pandora.com{p}")).unwrap_or_default();
    Ok(Show {
        key: id.to_string(),
        source: "pandora",
        title,
        author: program["publisherName"].as_str().unwrap_or_default().to_string(),
        art,
        description: String::new(),
        link,
        episodes,
    })
}

/// Pandora's shows for a search, with when each last had an episode.
fn pandora_search(pandora: &Pandora, query: &str) -> Result<Vec<Value>> {
    let found = pandora.call(
        "v3/sod/search",
        json!({ "query": query, "types": ["PC"], "listener": null, "start": 0, "count": 20, "annotate": true, "searchTime": 0, "annotationRecipe": "CLASS_OF_2019" }),
    )?;
    let annotations = found["annotations"].as_object().cloned().unwrap_or_default();
    let ids: Vec<String> = found["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_str().or(r["pandoraId"].as_str()).map(str::to_string))
        .filter(|id| id.starts_with("PC:"))
        .collect();
    let ids = if ids.is_empty() { annotations.keys().filter(|k| k.starts_with("PC:")).cloned().collect() } else { ids };
    let shows: Vec<&Value> = ids.iter().filter_map(|id| annotations.get(id)).collect();
    let latest_ids: Vec<String> = shows.iter().filter_map(|s| s["latestEpisodeId"].as_str().map(str::to_string)).collect();
    let latest = pandora_annotate(pandora, &latest_ids);
    Ok(shows
        .into_iter()
        .map(|s| {
            let newest = s["latestEpisodeId"].as_str().and_then(|e| latest.get(e)).and_then(|e| e["releaseDate"].as_str()).map(iso_time).unwrap_or(0);
            json!({
                "key": s["pandoraId"],
                "source": "pandora",
                "title": s["name"].as_str().unwrap_or_default(),
                "author": s["publisherName"].as_str().unwrap_or_default(),
                "art": pandora_art(&s["icon"]),
                "latest": newest,
            })
        })
        .collect())
}

// ---- Apple's directory: public, and it gives each show's feed

fn apple_show(r: &Value) -> Option<Value> {
    let url = r["feedUrl"].as_str().filter(|u| web_url(u))?;
    Some(json!({
        "key": url,
        "source": "feed",
        "title": r["collectionName"].as_str().unwrap_or_default(),
        "author": r["artistName"].as_str().unwrap_or_default(),
        "art": r["artworkUrl600"].as_str().or(r["artworkUrl100"].as_str()).unwrap_or_default(),
        "latest": iso_time(r["releaseDate"].as_str().unwrap_or_default()),
    }))
}

fn apple_search(query: &str) -> Result<Vec<Value>> {
    let found = get_json(HTTP.get("https://itunes.apple.com/search").query(&[("media", "podcast"), ("entity", "podcast"), ("limit", "30"), ("term", query)]))?;
    Ok(found["results"].as_array().into_iter().flatten().filter_map(apple_show).collect())
}

/// One entry per show: where it turns up more than once, the one with the
/// newest episode (a show that moved hosts leaves its old feed behind in
/// directories); on a tie, its own feed.
fn merge(lists: Vec<Vec<Value>>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for show in lists.into_iter().flatten() {
        let name = same_show(show["title"].as_str().unwrap_or_default());
        if name.is_empty() {
            continue;
        }
        let newer = |a: &Value, b: &Value| {
            let (la, lb) = (a["latest"].as_i64().unwrap_or(0), b["latest"].as_i64().unwrap_or(0));
            la > lb || (la == lb && a["source"] == "feed" && b["source"] != "feed")
        };
        match at.get(&name) {
            Some(&i) => {
                if newer(&show, &out[i]) {
                    out[i] = show;
                }
            }
            None => {
                at.insert(name, out.len());
                out.push(show);
            }
        }
    }
    out
}

/// Shows matching `query` from everywhere at once; a feed's own address
/// finds that show.
pub fn search(pandora: Option<&Pandora>, query: &str) -> Result<Value> {
    let query = query.trim();
    if web_url(query) {
        let show = feed(query)?;
        return Ok(json!([show.summary()]));
    }
    let lists: Vec<Result<Vec<Value>>> = std::thread::scope(|scope| {
        let from_pandora = scope.spawn(|| match pandora {
            Some(p) => pandora_search(p, query),
            None => Ok(Vec::new()),
        });
        let from_apple = scope.spawn(|| apple_search(query));
        [from_pandora, from_apple].into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow!("search failed")))).collect()
    });
    if lists.iter().all(|l| l.is_err()) {
        return Err(lists.into_iter().find_map(|l| l.err()).unwrap_or_else(|| anyhow!("search failed")));
    }
    Ok(json!(merge(lists.into_iter().filter_map(|l| l.ok()).collect())))
}

static TRENDING: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

/// Shows people are listening to now: Apple's chart.
pub fn trending() -> Result<Value> {
    if let Some((at, shows)) = TRENDING.lock().unwrap().as_ref()
        && at.elapsed() < TRENDING_KEEP
    {
        return Ok(shows.clone());
    }
    // The chart names shows by id; a lookup gives their feeds.
    let chart = get_json(HTTP.get("https://rss.marketingtools.apple.com/api/v2/us/podcasts/top/40/podcasts.json"))?;
    let ids: Vec<String> = chart["feed"]["results"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_str().map(str::to_string)).collect();
    if ids.is_empty() {
        return Ok(json!([]));
    }
    let found = get_json(HTTP.get("https://itunes.apple.com/lookup").query(&[("id", ids.join(",").as_str()), ("entity", "podcast")]))?;
    let by_id: HashMap<String, Value> = found["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some((r["collectionId"].as_u64()?.to_string(), apple_show(r)?)))
        .collect();
    let shows = json!(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect::<Vec<_>>());
    *TRENDING.lock().unwrap() = Some((Instant::now(), shows.clone()));
    Ok(shows)
}

// ---- shows

/// A show as its source has it, newest episode first.
pub struct Show {
    key: String,
    source: &'static str,
    title: String,
    author: String,
    art: String,
    description: String,
    link: String,
    episodes: Vec<Value>,
}

impl Show {
    fn summary(&self) -> Value {
        json!({
            "key": self.key,
            "source": self.source,
            "title": self.title,
            "author": self.author,
            "art": self.art,
            "latest": self.episodes.first().and_then(|e| e["published"].as_i64()).unwrap_or(0),
        })
    }
}

static SHOWS: LazyLock<Mutex<HashMap<String, (Instant, Arc<Show>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// A show by its key, read again after half an hour.
fn show_for(pandora: Option<&Pandora>, key: &str) -> Result<Arc<Show>> {
    if let Some((at, show)) = SHOWS.lock().unwrap().get(key)
        && at.elapsed() < FEED_KEEP
    {
        return Ok(show.clone());
    }
    let show = if key.starts_with("PC:") {
        let pandora = pandora.ok_or_else(|| anyhow!("Still signing in to Pandora."))?;
        Arc::new(pandora_show(pandora, key)?)
    } else {
        return feed(key);
    };
    SHOWS.lock().unwrap().insert(key.to_string(), (Instant::now(), show.clone()));
    Ok(show)
}

/// A show's RSS feed.
fn feed(url: &str) -> Result<Arc<Show>> {
    if let Some((at, show)) = SHOWS.lock().unwrap().get(url)
        && at.elapsed() < FEED_KEEP
    {
        return Ok(show.clone());
    }
    if !web_url(url) || !public_host(url) {
        bail!("That isn't a web address for a podcast feed.");
    }
    let response = HTTP
        .get(url)
        .send()
        .map_err(|e| anyhow!("Couldn't reach that feed ({}).", e.without_url()))?
        .error_for_status()
        .map_err(|e| anyhow!("That feed answered with an error ({}).", e.without_url()))?;
    let bytes = read_capped(response, FEED_MAX_BYTES).map_err(|_| anyhow!("That feed is too large."))?;
    let show = Arc::new(parse(url, &bytes)?);
    SHOWS.lock().unwrap().insert(url.to_string(), (Instant::now(), show.clone()));
    Ok(show)
}

/// What decodes it, from its type or its name.
fn encoding_of(kind: &str, url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    match kind {
        "audio/mpeg" | "audio/mp3" | "audio/mpeg3" => "mp3",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "m4a",
        "audio/aac" | "audio/aacp" => "aac",
        _ if path.ends_with(".m4a") || path.ends_with(".mp4") => "m4a",
        _ if path.ends_with(".aac") => "aac",
        _ => "mp3",
    }
}

fn audio_file(link: &str) -> bool {
    let path = link.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    [".mp3", ".m4a", ".mp4", ".aac"].iter().any(|ext| path.ends_with(ext))
}

/// Seconds from an iTunes duration: "3723", "62:03", or "1:02:03".
fn seconds(text: &str) -> Option<u64> {
    let mut total = 0f64;
    for part in text.trim().split(':') {
        total = total * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    (total > 0.0).then_some(total as u64)
}

/// Each episode's length by its audio's address, read from the XML: the
/// feed parser loses `itunes:duration` when it merges an item's media.
fn raw_durations(xml: &str) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    for item in xml.split("<item").skip(1) {
        let item = item.split("</item>").next().unwrap_or_default();
        let Some(enclosure) = item.find("<enclosure").map(|at| &item[at..]) else { continue };
        let url = enclosure.split("url=").nth(1).and_then(|rest| {
            let quote = rest.chars().next()?;
            rest[1..].split(quote).next()
        });
        let length = item.split("<itunes:duration>").nth(1).and_then(|rest| rest.split("</itunes:duration>").next()).and_then(seconds);
        if let (Some(url), Some(length)) = (url, length) {
            out.insert(url.replace("&amp;", "&"), length);
        }
    }
    out
}

fn parse(url: &str, bytes: &[u8]) -> Result<Show> {
    let lengths = raw_durations(&String::from_utf8_lossy(bytes));
    let parsed = feed_rs::parser::parse(bytes).map_err(|_| anyhow!("That address isn't a podcast feed."))?;
    let text = |t: &Option<feed_rs::model::Text>| t.as_ref().map(|t| strip_html(&t.content)).unwrap_or_default();
    let title = text(&parsed.title);
    let art = parsed.logo.as_ref().or(parsed.icon.as_ref()).map(|i| i.uri.clone()).filter(|u| web_url(u)).unwrap_or_default();
    let author = parsed.authors.first().and_then(|a| a.name.clone()).unwrap_or_default();
    let mut episodes = Vec::new();
    for entry in &parsed.entries {
        // The audio: an enclosure (or media content) that is sound.
        let found = entry.media.iter().find_map(|m| {
            m.content.iter().find_map(|c| {
                let link = c.url.as_ref()?.to_string();
                let kind = c.content_type.as_ref().map(|t| t.to_string()).unwrap_or_default();
                let audio = kind.starts_with("audio/") || (kind.is_empty() && audio_file(&link));
                (audio && web_url(&link)).then_some((link, kind, c.duration))
            })
        });
        let Some((enclosure, kind, duration)) = found else { continue };
        // The iTunes tags (length, picture) come as media of their own.
        let duration = duration.or_else(|| entry.media.iter().find_map(|m| m.duration)).map(|d| d.as_secs()).or_else(|| lengths.get(&enclosure).copied());
        let picture = entry.media.iter().find_map(|m| m.thumbnails.first().map(|t| t.image.uri.clone()));
        let guid = if entry.id.is_empty() { enclosure.clone() } else { entry.id.clone() };
        let published = entry.published.or(entry.updated).map(|d| d.timestamp()).unwrap_or(0);
        let summary = entry
            .summary
            .as_ref()
            .map(|s| s.content.clone())
            .or_else(|| entry.media.iter().find_map(|m| m.description.as_ref().map(|d| d.content.clone())))
            .or_else(|| entry.content.as_ref().and_then(|c| c.body.clone()))
            .unwrap_or_default();
        episodes.push(json!({
            "id": format!("EP:{}", hex_id(&format!("{url}\n{guid}"), 20)),
            "kind": "episode",
            "source": "feed",
            "title": text(&entry.title),
            "artist": title,
            "album": "",
            "show": url,
            "art": picture.filter(|p| web_url(p)).unwrap_or_else(|| art.clone()),
            "duration": duration,
            "published": published,
            "summary": shorten(&strip_html(&summary), SUMMARY_LENGTH),
            "playable": true,
            "encoding": encoding_of(&kind, &enclosure),
            "enclosure": enclosure,
        }));
    }
    if episodes.is_empty() && title.is_empty() {
        bail!("That address isn't a podcast feed.");
    }
    episodes.sort_by_key(|e| std::cmp::Reverse(e["published"].as_i64().unwrap_or(0)));
    // The show's website, not the feed's link to itself.
    let link = parsed.links.iter().find(|l| l.rel.as_deref() != Some("self") && l.href != url && web_url(&l.href)).map(|l| l.href.clone()).unwrap_or_default();
    Ok(Show { key: url.to_string(), source: "feed", title, author, art, description: text(&parsed.description), link, episodes })
}

// ---- your library: subscriptions, and where you are in each episode

/// One account's podcasts: whose, and what.
struct Library {
    owner: Option<String>,
    data: Value,
}

impl std::ops::Deref for Library {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.data
    }
}

impl std::ops::DerefMut for Library {
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.data
    }
}

static LIBRARY: Mutex<Library> = Mutex::new(Library { owner: None, data: Value::Null });

/// The signed-in account's podcasts, read again whenever the account
/// changes. Signed out, an empty library that isn't saved.
fn library() -> std::sync::MutexGuard<'static, Library> {
    let mut library = LIBRARY.lock().unwrap();
    let user = crate::pandora::saved_user();
    if library.data.is_null() || library.owner != user {
        let stored = user.as_deref().and_then(|u| std::fs::read_to_string(library_path(u)).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let mut data = stored.filter(Value::is_object).unwrap_or_else(|| json!({}));
        if !data["subscriptions"].is_array() {
            data["subscriptions"] = json!([]);
        }
        if !data["episodes"].is_object() {
            data["episodes"] = json!({});
        }
        *library = Library { owner: user, data };
    }
    library
}

fn save(library: &Library) {
    if let Some(user) = &library.owner {
        let _ = crate::files::account_dir();
        let _ = crate::files::write_private(&library_path(user), serde_json::to_string_pretty(&library.data).unwrap_or_default().as_bytes());
    }
}

// ---- following: on Pandora for any show it has, here for the rest

static FOLLOWS: Mutex<Option<(Option<String>, Instant, Vec<Value>)>> = Mutex::new(None);
const FOLLOWS_KEEP: Duration = Duration::from_secs(60);

/// The podcasts followed on Pandora (its collection, which has them only
/// in its newer form), as shows; kept a minute.
fn pandora_follows(pandora: &Pandora) -> Result<Vec<Value>> {
    let user = crate::pandora::saved_user();
    if let Some((owner, at, shows)) = FOLLOWS.lock().unwrap().as_ref()
        && *owner == user
        && at.elapsed() < FOLLOWS_KEEP
    {
        return Ok(shows.clone());
    }
    let mut ids: Vec<String> = Vec::new();
    let mut cursor = String::new();
    for _ in 0..50 {
        let mut request = json!({ "limit": 100 });
        if !cursor.is_empty() {
            request["cursor"] = json!(cursor);
        }
        let page = pandora.call("v6/collections/getItems", json!({ "request": request }))?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        ids.extend(items.iter().filter(|i| i["pandoraType"] == "PC").filter_map(|i| i["pandoraId"].as_str().map(str::to_string)));
        let next = page["cursor"].as_str().unwrap_or_default().to_string();
        if items.is_empty() || next.is_empty() || next == cursor {
            break;
        }
        cursor = next;
    }
    let details = pandora_annotate(pandora, &ids);
    let shows: Vec<Value> = ids
        .iter()
        .filter_map(|id| details.get(id))
        .map(|a| json!({ "key": a["pandoraId"], "source": "pandora", "title": a["name"], "author": a["publisherName"], "art": pandora_art(&a["icon"]) }))
        .collect();
    *FOLLOWS.lock().unwrap() = Some((user, Instant::now(), shows.clone()));
    Ok(shows)
}

fn pandora_follow(pandora: &Pandora, id: &str, add: bool) -> Result<()> {
    let path = if add { "v6/collections/addItem" } else { "v6/collections/removeItem" };
    pandora.call(path, json!({ "request": { "pandoraId": id } }))?;
    *FOLLOWS.lock().unwrap() = None;
    Ok(())
}

/// Pandora's own copy of a show found elsewhere, by its name.
fn on_pandora(pandora: &Pandora, title: &str) -> Option<String> {
    let wanted = same_show(title);
    pandora_search(pandora, title).ok()?.into_iter().find(|s| same_show(s["title"].as_str().unwrap_or_default()) == wanted).and_then(|s| s["key"].as_str().map(str::to_string))
}

/// Your shows: those followed on Pandora, and the feeds followed here.
/// One show followed both ways is listed once, as its feed (which Omadora
/// chose for having the newest episodes).
pub fn subscriptions(pandora: Option<&Pandora>) -> Value {
    let mine: Vec<Value> = library()["subscriptions"].as_array().cloned().unwrap_or_default();
    let names: std::collections::HashSet<String> = mine.iter().map(|s| same_show(s["title"].as_str().unwrap_or_default())).collect();
    let mut shows = mine;
    if let Some(pandora) = pandora
        && let Ok(theirs) = pandora_follows(pandora)
    {
        shows.extend(theirs.into_iter().filter(|s| !names.contains(&same_show(s["title"].as_str().unwrap_or_default()))));
    }
    json!(shows)
}

fn subscribed(pandora: Option<&Pandora>, key: &str, title: &str) -> bool {
    let name = same_show(title);
    subscriptions(pandora).as_array().into_iter().flatten().any(|s| s["key"] == key || same_show(s["title"].as_str().unwrap_or_default()) == name)
}

/// Follow a show, or stop. A show Pandora has is followed there too, so
/// Pandora's apps list it; one it hasn't is followed here. Following reads
/// the show first, so a mistyped feed address says so instead of sitting
/// in the list.
pub fn subscribe(pandora: Option<Arc<Pandora>>, key: &str, add: bool) -> Result<Value> {
    let pandora = pandora.as_deref();
    let key = key.trim();
    let show = show_for(pandora, key)?;
    let name = same_show(&show.title);
    if add {
        let on_pandora = if key.starts_with("PC:") { Some(key.to_string()) } else { pandora.and_then(|p| on_pandora(p, &show.title)) };
        if let (Some(p), Some(id)) = (pandora, &on_pandora) {
            pandora_follow(p, id, true)?;
        }
        // A feed is kept here, Pandora or not: it's the one that plays.
        if !key.starts_with("PC:") {
            let mut library = library();
            let list = library["subscriptions"].as_array_mut().expect("an array");
            list.retain(|s| s["key"] != key);
            let mut entry = show.summary();
            entry["added"] = json!(now());
            list.insert(0, entry);
            save(&library);
        }
    } else {
        // Every way it's followed: here, and on Pandora by id or by name.
        {
            let mut library = library();
            let list = library["subscriptions"].as_array_mut().expect("an array");
            list.retain(|s| s["key"] != key && same_show(s["title"].as_str().unwrap_or_default()) != name);
            save(&library);
        }
        if let Some(p) = pandora {
            let theirs = pandora_follows(p).unwrap_or_default();
            for followed in theirs.iter().filter(|s| s["key"] == key || same_show(s["title"].as_str().unwrap_or_default()) == name) {
                if let Some(id) = followed["key"].as_str() {
                    pandora_follow(p, id, false)?;
                }
            }
        }
    }
    Ok(subscriptions(pandora))
}

/// Where you are in an episode, and whether you've played it.
fn progress_of(id: &str) -> (f64, bool) {
    let library = library();
    let e = &library["episodes"][id];
    (e["position"].as_f64().unwrap_or(0.0), e["played"].as_bool().unwrap_or(false))
}

fn with_progress(mut episode: Value) -> Value {
    let id = episode["id"].as_str().unwrap_or_default().to_string();
    let (position, played) = progress_of(&id);
    episode["position"] = json!(position);
    episode["played"] = json!(played);
    if let Some(map) = episode.as_object_mut() {
        map.remove("enclosure");
    }
    episode
}

/// What History shows of an episode.
fn keepsake(episode: &Value) -> Value {
    let mut kept = json!({});
    for field in ["id", "kind", "source", "title", "artist", "show", "art", "duration", "published"] {
        kept[field] = episode[field].clone();
    }
    kept
}

/// The engine, every few seconds while an episode plays.
pub fn remember(episode: &Value, position: f64) {
    let Some(id) = episode["id"].as_str() else { return };
    let finished = episode["duration"].as_f64().is_some_and(|d| d > 0.0 && position >= d - END_SLACK);
    let mut library = library();
    let played = finished || library["episodes"][id]["played"].as_bool().unwrap_or(false);
    library["episodes"][id] = json!({ "position": if finished { 0.0 } else { position }, "played": played, "at": now(), "episode": keepsake(episode) });
    save(&library);
}

/// Mark an episode played (it ended, or from its row) or not.
pub fn mark(id: &str, played: bool) {
    let mut library = library();
    let kept = library["episodes"][id]["episode"].clone();
    library["episodes"][id] = json!({ "position": 0.0, "played": played, "at": now(), "episode": kept });
    save(&library);
}

/// Episodes listened to lately, newest first, for History.
pub fn recent(limit: usize) -> Vec<Value> {
    let library = library();
    let mut heard: Vec<(u64, Value)> = library["episodes"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, e)| e["episode"].is_object())
        .map(|(_, e)| {
            let mut episode = e["episode"].clone();
            episode["position"] = e["position"].clone();
            episode["played"] = e["played"].clone();
            (e["at"].as_u64().unwrap_or(0), episode)
        })
        .collect();
    heard.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    heard.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// Where to start an episode: where you left it, unless it's done.
pub fn start_at(id: &str) -> f64 {
    let (position, played) = progress_of(id);
    if played { 0.0 } else { position }
}

// ---- what the widget reads

/// Your shows' newest episodes, newest first: the Podcasts tab's list, and
/// Up Next after the episode playing.
fn new_episodes(pandora: Option<&Pandora>) -> Vec<Value> {
    let keys: Vec<String> = subscriptions(pandora).as_array().into_iter().flatten().filter_map(|s| s["key"].as_str().map(str::to_string)).collect();
    let mut latest: Vec<Value> = std::thread::scope(|scope| {
        let handles: Vec<_> = keys.iter().map(|key| scope.spawn(move || show_for(pandora, key).ok())).collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .flat_map(|show| show.episodes.iter().take(5).cloned().collect::<Vec<_>>())
            .collect()
    });
    latest.sort_by_key(|e| std::cmp::Reverse(e["published"].as_i64().unwrap_or(0)));
    latest.truncate(30);
    latest
}

/// The Podcasts tab: your shows, and their newest episodes.
pub fn overview(pandora: Option<Arc<Pandora>>) -> Value {
    json!({
        "subscriptions": subscriptions(pandora.as_deref()),
        "latest": new_episodes(pandora.as_deref()).into_iter().map(with_progress).collect::<Vec<_>>(),
    })
}

/// A show's page.
pub fn show(pandora: Option<Arc<Pandora>>, key: &str) -> Result<Value> {
    let show = show_for(pandora.as_deref(), key.trim())?;
    Ok(json!({
        "key": show.key,
        "source": show.source,
        "title": show.title,
        "author": show.author,
        "art": show.art,
        "description": show.description,
        "link": show.link,
        "subscribed": subscribed(pandora.as_deref(), &show.key, &show.title),
        "count": show.episodes.len(),
        "episodes": show.episodes.iter().take(PAGE_EPISODES).cloned().map(with_progress).collect::<Vec<_>>(),
    }))
}

// ---- playing

/// The queue for an episode of a show (`podcast:KEY`): that episode, then
/// your new episodes you haven't played, as the Podcasts tab lists them.
pub fn queue_for(pandora: Option<&Pandora>, source: &str, episode: Option<&str>) -> Result<(String, Vec<Value>)> {
    let key = source.strip_prefix("podcast:").ok_or_else(|| anyhow!("not a podcast"))?;
    let show = show_for(pandora, key)?;
    let first = episode
        .and_then(|id| show.episodes.iter().find(|e| e["id"] == id))
        .or(show.episodes.first())
        .cloned()
        .ok_or_else(|| anyhow!("This show has no episodes."))?;
    let mut queue = vec![first.clone()];
    queue.extend(new_episodes(pandora).into_iter().filter(|e| e["id"] != first["id"] && !progress_of(e["id"].as_str().unwrap_or_default()).1));
    Ok((show.title.clone(), queue))
}

/// A show by its name (and, to tell namesakes apart, its host), from
/// Pandora or Apple: for the AI tab's suggestions.
pub fn find_show(pandora: Option<&Pandora>, title: &str, host: &str) -> Option<Value> {
    let found = search(pandora, title).ok()?;
    let wanted = same_show(title);
    let host = same_show(host);
    let shows = found.as_array()?;
    let named: Vec<&Value> = shows.iter().filter(|s| same_show(s["title"].as_str().unwrap_or_default()) == wanted).collect();
    named
        .iter()
        .find(|s| !host.is_empty() && same_show(s["author"].as_str().unwrap_or_default()).contains(&host))
        .or(named.first())
        .map(|s| (*s).clone())
}

/// Where an episode's audio is: its enclosure, or (Pandora's) what Pandora
/// hands out for it.
fn audio_url(pandora: Option<&Pandora>, episode: &Value) -> Result<String> {
    if episode["source"] == "pandora" {
        let pandora = pandora.ok_or_else(|| anyhow!("Still signing in to Pandora."))?;
        let info = pandora.call(
            "v1/ondemand/getAudioPlaybackInfo",
            json!({ "pandoraId": episode["id"], "sourcePandoraId": episode["show"], "deviceCode": "", "includeAudioToken": true }),
        )?;
        // The MP3 of it: the default is HE-AAC, which needs ffmpeg whole.
        let map = &info["audioUrlMap"];
        return ["highQuality", "mediumQuality", "lowQuality"]
            .iter()
            .map(|q| &map[q])
            .find(|q| q["encoding"] == "mp3")
            .and_then(|q| q["audioUrl"].as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Pandora has no audio for this episode."));
    }
    episode["enclosure"].as_str().map(str::to_string).ok_or_else(|| anyhow!("This episode has no audio."))
}

/// Start an episode's download, and wait until there's enough to play
/// from `start` seconds in.
pub fn open(pandora: Option<&Pandora>, episode: &Value, start: f64) -> Result<Arc<Download>> {
    let url = audio_url(pandora, episode)?;
    if !web_url(&url) || !public_host(&url) {
        bail!("This episode's audio isn't on the web.");
    }
    let dir = cache_dir();
    let id = episode["id"].as_str().unwrap_or(&url);
    let path = dir.join(format!("{}.{}", hex_id(id, 24), episode["encoding"].as_str().unwrap_or("mp3")));
    crate::stream::prune(&dir, CACHE_MAX, &path);
    let download = Download::start(AUDIO_HTTP.clone(), &url, path).map_err(|e| anyhow!("Couldn't get this episode: {e:#}"))?;
    // MP3 plays from its first bytes; an M4A may keep its index at the
    // end, so it waits for the whole file.
    let (wanted, patience) = if episode["encoding"] == "mp3" {
        (bytes_for(&download, episode, start).unwrap_or(START_BYTES), Duration::from_secs(120))
    } else {
        (u64::MAX, Duration::from_secs(600))
    };
    if !download.wait_for(wanted, patience) {
        bail!("{}", download.failure().unwrap_or_else(|| "This episode is slow to download; try again.".into()));
    }
    Ok(download)
}

/// Bytes to have before playing from `seconds` into an episode.
pub fn bytes_for(download: &Download, episode: &Value, seconds: f64) -> Option<u64> {
    if seconds <= 0.0 {
        return Some(START_BYTES);
    }
    let total = download.total()? as f64;
    let length = episode["duration"].as_f64().filter(|d| *d > 0.0)?;
    Some((total * (seconds / length).clamp(0.0, 1.0)) as u64 + START_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodes_come_from_the_feed() {
        let xml = br#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel><title>A Show</title><itunes:author>Someone</itunes:author>
<itunes:image href="https://example.com/show.jpg"/>
<item><title>Older</title><guid>a</guid><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
<enclosure url="https://example.com/a.mp3" type="audio/mpeg" length="1"/><itunes:duration>1:02:03</itunes:duration></item>
<item><title>Newer</title><guid>b</guid><pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
<enclosure url="https://example.com/b.m4a" type="audio/x-m4a" length="1"/></item>
<item><title>No audio</title><guid>c</guid></item>
</channel></rss>"#;
        let show = parse("https://example.com/feed", xml).unwrap();
        assert_eq!(show.source, "feed");
        assert_eq!(show.title, "A Show");
        assert_eq!(show.art, "https://example.com/show.jpg");
        assert_eq!(show.episodes.len(), 2);
        assert_eq!(show.episodes[0]["title"], "Newer");
        assert_eq!(show.episodes[0]["encoding"], "m4a");
        assert_eq!(show.episodes[1]["duration"], 3723);
        assert_eq!(show.episodes[1]["artist"], "A Show");
        assert!(show.episodes[0]["id"].as_str().unwrap().starts_with("EP:"));
        assert_ne!(show.episodes[0]["id"], show.episodes[1]["id"]);
    }

    #[test]
    fn iso_times_to_seconds() {
        assert_eq!(iso_time("1970-01-01T00:00:00Z"), 0);
        assert_eq!(iso_time("2026-10-13T09:00:00Z"), iso_time("2026-10-13T02:00:00.000-07:00"));
        assert_eq!(iso_time("2024-01-02T00:00:00Z") - iso_time("2024-01-01T00:00:00Z"), 86400);
        assert_eq!(iso_time("junk"), 0);
    }

    #[test]
    fn one_entry_per_show_the_freshest() {
        let old_feed = json!({ "key": "https://old/feed", "source": "feed", "title": "Things I Learned Last Night", "latest": 100 });
        let pandora = json!({ "key": "PC:1", "source": "pandora", "title": "Things I Learned Last Night", "latest": 500 });
        let new_feed = json!({ "key": "https://new/feed", "source": "feed", "title": "Things I learned last night", "latest": 500 });
        let other = json!({ "key": "PC:2", "source": "pandora", "title": "Another", "latest": 1 });
        let merged = merge(vec![vec![pandora, other], vec![old_feed], vec![new_feed]]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["key"], "https://new/feed");
        assert_eq!(merged[1]["key"], "PC:2");
    }

    #[test]
    fn itunes_durations() {
        assert_eq!(seconds("3723"), Some(3723));
        assert_eq!(seconds("62:03"), Some(3723));
        assert_eq!(seconds("1:02:03"), Some(3723));
        assert_eq!(seconds("soon"), None);
    }

    #[test]
    fn each_account_has_its_own_library() {
        let a = library_path("someone@example.com");
        assert_eq!(a, library_path(" Someone@Example.com "));
        assert_ne!(a, library_path("someone.else@example.com"));
        assert!(!a.to_string_lossy().contains("example"));
        assert!(a.ends_with("podcasts.json"));
    }

    #[test]
    fn audio_types() {
        assert_eq!(encoding_of("audio/mpeg", "https://x/a"), "mp3");
        assert_eq!(encoding_of("", "https://x/a.m4a?track=1"), "m4a");
        assert_eq!(encoding_of("audio/aac", "https://x/a"), "aac");
    }
}
