//! Discover: music magazines' RSS feeds, each article matched to the album,
//! song, or artist it is about on Pandora.
//!
//! Feeds come from `~/.config/omadora/feeds.toml`, or Pitchfork's reviews,
//! Rolling Stone's music news, and Music Connection when that file is absent:
//!
//! ```toml
//! [[feed]]
//! name = "Pitchfork: Best New Albums"
//! url = "https://pitchfork.com/feed/reviews/best/albums/rss"
//! match = "pitchfork"
//! ```
//!
//! `match` says how to tell what an article is about:
//!   pitchfork  the title is the album or song; the link adds the artist
//!   title      titles like "Artist – Album" or "Artist, “Album”"
//!   tags       the article's categories name artists
//!   headline   the headline starts with an artist's name
//!   none       show the article, match nothing
//!
//! A new magazine with its own style needs one more arm in `leads`.
//!
//! A match counts only when Pandora's title and artist both agree with the
//! article, so an article never links to the wrong album. Matches are cached
//! on disk, so each article is looked up once; misses are tried again when the
//! engine restarts, in case Pandora has added the record since.
//!
//! Feeds are fetched at most every half hour, and conditionally: a magazine
//! that sends an ETag or Last-Modified answers an unchanged feed with an
//! empty 304. Article pictures are downloaded once, when the widget first
//! shows them, and kept in `~/.cache/omadora/images` while in use.

use crate::pandora::{Pandora, USER_AGENT};
use crate::catalog;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

const REFRESH: Duration = Duration::from_secs(30 * 60);
const SUMMARY_LENGTH: usize = 280;

const DEFAULT_FEEDS: [(&str, &str, Matcher); 6] = [
    ("Pitchfork: Best New Albums", "https://pitchfork.com/feed/reviews/best/albums/rss", Matcher::Pitchfork),
    ("Pitchfork: Best New Tracks", "https://pitchfork.com/feed/reviews/best/tracks/rss", Matcher::Pitchfork),
    ("Pitchfork: Album Reviews", "https://pitchfork.com/feed/feed-album-reviews/rss", Matcher::Pitchfork),
    ("Pitchfork: Track Reviews", "https://pitchfork.com/feed/feed-track-reviews/rss", Matcher::Pitchfork),
    ("Rolling Stone", "https://www.rollingstone.com/music/feed/", Matcher::Tags),
    ("Music Connection", "https://www.musicconnection.com/feed/", Matcher::Headline),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Matcher {
    Pitchfork,
    Title,
    Tags,
    Headline,
    None,
}

impl Matcher {
    fn parse(name: &str) -> Self {
        match name.trim().to_lowercase().as_str() {
            "pitchfork" => Matcher::Pitchfork,
            "title" => Matcher::Title,
            "tags" => Matcher::Tags,
            "headline" => Matcher::Headline,
            _ => Matcher::None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Feed {
    pub name: String,
    pub url: String,
    pub matcher: Matcher,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config"));
    base.join("omadora").join("feeds.toml")
}

fn cache_path() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".local/state"));
    base.join("omadora").join("discover.json")
}

/// The configured feeds, or the Pitchfork defaults.
pub fn feeds() -> Vec<Feed> {
    let configured = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("feed").and_then(|f| f.as_array()).cloned())
        .map(|list| {
            list.iter()
                .filter_map(|f| {
                    let url = f.get("url")?.as_str()?.to_string();
                    let name = f.get("name").and_then(|n| n.as_str()).unwrap_or(&url).to_string();
                    let matcher = Matcher::parse(f.get("match").and_then(|m| m.as_str()).unwrap_or("none"));
                    Some(Feed { name, url, matcher })
                })
                .collect::<Vec<_>>()
        })
        .filter(|list| !list.is_empty());
    configured.unwrap_or_else(|| {
        DEFAULT_FEEDS
            .iter()
            .map(|(name, url, matcher)| Feed { name: name.to_string(), url: url.to_string(), matcher: *matcher })
            .collect()
    })
}

// ---- reading feeds

/// One article, as the widget shows it, plus what is needed to match it.
#[derive(Clone, Debug)]
struct Article {
    id: String,
    feed: String,
    matcher: Matcher,
    title: String,
    summary: String,
    link: String,
    published: i64,
    image: String,
    categories: Vec<String>,
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let decoded = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&rsquo;", "’")
        .replace("&lsquo;", "‘")
        .replace("&rdquo;", "”")
        .replace("&ldquo;", "“")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end_matches([',', ';', ':', '.']))
}

/// The text of the first `attr="…"` inside the first `tag` in `chunk`.
fn attr_of(chunk: &str, tag: &str, attr: &str) -> Option<String> {
    let start = chunk.find(tag)?;
    let rest = &chunk[start..];
    let element = &rest[..rest.find('>')?];
    let key = format!("{attr}=\"");
    let from = element.find(&key)? + key.len();
    let len = element[from..].find('"')?;
    Some(element[from..from + len].replace("&amp;", "&"))
}

/// Item images straight from the XML, by link. feed-rs drops some feeds'
/// `<media:thumbnail>` (Pitchfork's among them), so look for it directly.
fn raw_images(xml: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for chunk in xml.split("<item").skip(1) {
        let Some(link) = chunk
            .find("<link>")
            .and_then(|i| chunk[i + 6..].find("</link>").map(|j| &chunk[i + 6..i + 6 + j]))
            .map(|l| l.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim().to_string())
        else {
            continue;
        };
        let image = attr_of(chunk, "<media:thumbnail", "url")
            .or_else(|| attr_of(chunk, "<media:content", "url"))
            .or_else(|| {
                let enclosure = &chunk[chunk.find("<enclosure")?..];
                attr_of(enclosure, "<enclosure", "type")
                    .filter(|t| t.starts_with("image"))
                    .and_then(|_| attr_of(enclosure, "<enclosure", "url"))
            });
        if let Some(image) = image {
            out.insert(link, image);
        }
    }
    out
}

/// Pitchfork serves full-size originals (2500 px); its image server sizes
/// them down when asked.
fn card_size(url: String) -> String {
    if url.contains("media.pitchfork.com") && url.contains("/master/pass/") {
        url.replace("/master/pass/", "/master/w_640,c_limit/")
    } else {
        url
    }
}

/// A feed as last fetched, with what the server needs to say "unchanged".
#[derive(Clone, Default)]
struct Fetched {
    etag: Option<String>,
    modified: Option<String>,
    articles: Vec<Article>,
}

fn read_feed(http: &reqwest::blocking::Client, feed: &Feed, last: Option<&Fetched>) -> anyhow::Result<Fetched> {
    let mut request = http.get(&feed.url);
    if let Some(last) = last {
        if let Some(etag) = &last.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(modified) = &last.modified {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, modified);
        }
    }
    let response = request.send().map_err(reqwest::Error::without_url)?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(last) = last {
            return Ok(last.clone());
        }
    }
    let response = response.error_for_status()?;
    let header = |name| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (etag, modified) = (header(reqwest::header::ETAG), header(reqwest::header::LAST_MODIFIED));
    let articles = parse_feed(feed, &read_capped(response, FEED_MAX_BYTES)?)?;
    Ok(Fetched { etag, modified, articles })
}

fn parse_feed(feed: &Feed, bytes: &[u8]) -> anyhow::Result<Vec<Article>> {
    let fallback_images = raw_images(&String::from_utf8_lossy(bytes));
    let parsed = feed_rs::parser::parse(bytes)?;
    Ok(parsed
        .entries
        .into_iter()
        .filter_map(|e| {
            // Only web links: the widget opens them, and a feed could name
            // a file or another app's URL scheme instead.
            let link = e.links.first().map(|l| l.href.clone()).filter(|l| web_url(l)).unwrap_or_default();
            let title = e.title.map(|t| strip_html(&t.content)).unwrap_or_default();
            if title.is_empty() || link.is_empty() {
                return None;
            }
            let image = e
                .media
                .iter()
                .flat_map(|m| m.thumbnails.iter().map(|t| t.image.uri.clone()))
                .chain(e.media.iter().flat_map(|m| {
                    m.content
                        .iter()
                        .filter(|c| c.content_type.as_ref().is_some_and(|t| t.to_string().starts_with("image")))
                        .filter_map(|c| c.url.as_ref().map(|u| u.to_string()))
                }))
                .next()
                .or_else(|| fallback_images.get(&link).cloned())
                .filter(|i| web_url(i) && public_host(i))
                .map(card_size)
                .unwrap_or_default();
            Some(Article {
                id: if e.id.is_empty() { link.clone() } else { e.id },
                feed: feed.name.clone(),
                matcher: feed.matcher,
                summary: shorten(&e.summary.map(|s| strip_html(&s.content)).unwrap_or_default(), SUMMARY_LENGTH),
                published: e.published.or(e.updated).map(|d| d.timestamp_millis()).unwrap_or(0),
                image,
                categories: e.categories.into_iter().map(|c| c.term).collect(),
                title,
                link,
            })
        })
        .collect())
}

// ---- telling what an article is about

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Lead {
    Album { title: String, artist: String },
    Track { title: String, artist: String },
    Artist { name: String },
    /// An artist named at the start of `text`, and mentioned in `body` too.
    Headline { text: String, body: String },
}

/// Lowercase ASCII words, for comparing names written different ways.
fn words(text: &str) -> Vec<String> {
    words_as_written(text).iter().map(|w| w.to_lowercase()).collect()
}

/// `words`, keeping each word's capitals.
fn words_as_written(text: &str) -> Vec<String> {
    let mut out = String::new();
    for ch in text.chars() {
        let ch = match ch {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        };
        if ch.is_alphanumeric() {
            out.push(ch);
        } else if ch == '\'' || ch == '’' || ch == '.' {
            // "Don't" and "D.C." keep their letters together.
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().map(str::to_string).collect()
}

fn same(a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    !a.is_empty() && a == b
}

/// Strip quotes and "[ft. …]" from a song or album title.
fn clean_title(title: &str) -> String {
    let mut out = title.to_string();
    if let Some(i) = out.find(" [") {
        out.truncate(i);
    }
    out.trim().trim_matches(['“', '”', '"', '‘', '’', '\'']).trim().to_string()
}

/// Does Pandora's album or song name match the one wanted? Pandora adds
/// credits and editions the magazine leaves off: "Lies (feat. Lana Del Rey)",
/// "Dopamine Chamber (Deluxe)".
/// The bracketed extras Pandora's name adds to the wanted title, outermost
/// last: `Some([])` for the same title, `None` when they differ.
fn extras(pandora_name: &str, wanted: &str) -> Option<Vec<String>> {
    let mut name = pandora_name.trim();
    let mut added = Vec::new();
    loop {
        if same(name, wanted) {
            return Some(added);
        }
        name.ends_with([')', ']']).then_some(())?;
        let start = name.rfind(['(', '['])?;
        added.push(name[start..].to_string());
        name = name[..start].trim_end();
    }
}

/// Extras that leave the recording itself alone: a remaster, a featured
/// artist, an edition. "(Moby Remix)" or "(Blue Silver)" is another take.
fn harmless(extra: &str) -> bool {
    const WORDS: [&str; 14] = [
        "remaster", "remastered", "deluxe", "edition", "expanded", "anniversary", "version", "single", "album",
        "original", "mono", "stereo", "bonus", "track",
    ];
    let w = words(extra);
    matches!(w.first().map(String::as_str), Some("feat" | "ft" | "featuring" | "with"))
        || w.iter().all(|x| WORDS.contains(&x.as_str()) || x.chars().all(|c| c.is_ascii_digit()))
}

/// Pitchfork review links are the artist then the title:
/// /reviews/albums/rachel-chinouriri-i-think-i-spoke-too-soon/
fn pitchfork_artist(link: &str, title: &str) -> String {
    let slug = link.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let title_slug = words(title).join("-");
    match slug.strip_suffix(&title_slug) {
        Some(artist) if !title_slug.is_empty() => artist.trim_end_matches('-').replace('-', " "),
        _ => String::new(),
    }
}

/// Words magazines use for their sections. A tag made only of these
/// ("Music News", "Live Reviews", "industry") is a section, not an artist.
const SECTION_WORDS: [&str; 26] = [
    "music", "news", "reviews", "review", "features", "feature", "interviews", "interview", "lists", "list", "video",
    "videos", "live", "industry", "album", "albums", "song", "songs", "of", "the", "day", "essential", "releases",
    "best", "new", "premieres",
];

/// Tags that may name artists: not sections, and the ones the headline
/// mentions first (magazines often list tags alphabetically, so the subject
/// of "See Red Hot Chili Peppers, Dave Grohl Perform…" can come last).
fn artist_tags(a: &Article) -> Vec<String> {
    let title = words(&a.title).join(" ");
    let mut tags: Vec<(bool, &String)> = a
        .categories
        .iter()
        .filter(|c| !c.contains('/'))
        .filter(|c| !words(c).iter().all(|w| SECTION_WORDS.contains(&w.as_str())))
        .map(|c| (!title.contains(&words(c).join(" ")), c))
        .collect();
    tags.sort_by_key(|(not_in_title, _)| *not_in_title);
    tags.into_iter().take(4).map(|(_, c)| c.trim().to_string()).collect()
}

fn leads(a: &Article) -> Vec<Lead> {
    match a.matcher {
        Matcher::Pitchfork => {
            let title = clean_title(&a.title);
            let artist = pitchfork_artist(&a.link, &title);
            let is_track = a.link.contains("/reviews/track") || a.categories.iter().any(|c| c.contains("Tracks"));
            let is_album = a.link.contains("/reviews/albums/") || a.categories.iter().any(|c| c.contains("Albums"));
            if is_track {
                vec![Lead::Track { title, artist }]
            } else if is_album {
                vec![Lead::Album { title, artist }]
            } else {
                Vec::new()
            }
        }
        Matcher::Title => {
            let t = &a.title;
            let split = [" – ", " — ", " - "].iter().find_map(|sep| t.split_once(sep)).or_else(|| t.split_once(", “"));
            match split {
                Some((artist, album)) => {
                    vec![Lead::Album { title: clean_title(album), artist: artist.trim().to_string() }]
                }
                None => Vec::new(),
            }
        }
        Matcher::Tags => artist_tags(a).into_iter().map(|name| Lead::Artist { name }).collect(),
        Matcher::Headline => {
            // "My Favorite Album: Jaïr Faria Lives In Rainbows" is about Jaïr
            // Faria; what comes before the colon is the column's name.
            let text = a.title.rsplit_once(": ").map_or(a.title.as_str(), |(_, rest)| rest).trim().to_string();
            vec![Lead::Headline { text, body: a.summary.clone() }]
        }
        Matcher::None => Vec::new(),
    }
}

/// Does Pandora's artist credit agree with the article's hint? Either side
/// may name more people ("Olivia Rodrigo & Chappell Roan" for a hint of
/// "olivia rodrigo chappell roan"), so it is enough for one to contain the
/// other's leading name.
fn artist_agrees(pandora_artist: &str, hint: &str) -> bool {
    let hint = words(hint);
    if hint.is_empty() {
        return false;
    }
    let credit = words(pandora_artist);
    let lead = words(pandora_artist.split(['&', ',']).next().unwrap_or_default());
    let contains = |big: &[String], small: &[String]| !small.is_empty() && small.iter().all(|w| big.contains(w));
    contains(&hint, &lead) || contains(&credit, &hint)
}

/// The artist a headline starts with, among those Pandora's search found:
/// the longest name the headline begins with that the article's text also
/// mentions, written as a name. A pun on a longer name ("Rage Against the
/// Rockstars") matches nothing rather than the shorter one ("Rage").
fn headline_artist(text: &str, body: &str, found: &[Value]) -> Option<Value> {
    let said = words(text);
    let body = words_as_written(body);
    let shared = |name: &[String]| name.iter().zip(&said).take_while(|(a, b)| a == b).count();
    // "Electronic pioneers Orbital recall…" names Orbital; "PROPS On October
    // 10…" and "the art of film" are a section label and a word.
    let mentions = |name: &str| {
        let (lower, written) = (words(name), words_as_written(name));
        body.windows(lower.len()).any(|w| {
            let joined = w.concat();
            w.iter().map(|x| x.to_lowercase()).eq(lower.iter().cloned())
                && (w == written.as_slice()
                    || joined.chars().any(char::is_uppercase) && joined.chars().any(char::is_lowercase))
        })
    };
    let mut best: Option<(usize, &Value)> = None;
    for artist in found {
        let written = artist["name"].as_str().unwrap_or_default();
        let name = words(written);
        if !name.is_empty() && shared(&name) == name.len() && mentions(written) && best.is_none_or(|(n, _)| name.len() > n) {
            best = Some((name.len(), artist));
        }
    }
    let (length, artist) = best?;
    let pun = found.iter().any(|a| {
        let name = words(a["name"].as_str().unwrap_or_default());
        shared(&name) > length && shared(&name) < name.len()
    });
    (!pun).then(|| artist.clone())
}

/// The search result for a title and artist: the exact title if there is
/// one, else one whose extras leave the recording alone ("(feat. …)",
/// "(2001 Remaster)"), and a remix only when nothing else matches.
fn best<'a>(found: &'a [Value], field: &str, title: &str, artist: &str) -> Option<&'a Value> {
    let by = |x: &&Value| artist_agrees(x["artist"].as_str().unwrap_or_default(), artist);
    let name = |x: &Value| x[field].as_str().unwrap_or_default().to_string();
    // Exact, then only harmless extras, then any extras.
    let rank = |x: &&Value| match extras(&name(x), title)? {
        e if e.is_empty() => Some(0),
        e if e.iter().all(|x| harmless(x)) => Some(1),
        _ => Some(2),
    };
    found.iter().filter(by).filter_map(|x| rank(&x).map(|r| (r, x))).min_by_key(|(r, _)| *r).map(|(_, x)| x)
}

pub(crate) fn resolve(pandora: &Pandora, lead: &Lead) -> Option<Value> {
    match lead {
        Lead::Album { title, artist } => {
            let found = catalog::search(pandora, &format!("{artist} {title}")).ok()?;
            let album = best(found["albums"].as_array()?, "name", title, artist)?;
            let mut m = album.clone();
            m["kind"] = json!("album");
            Some(m)
        }
        Lead::Track { title, artist } => {
            let found = catalog::search(pandora, &format!("{artist} {title}")).ok()?;
            let track = best(found["tracks"].as_array()?, "title", title, artist)?;
            let mut m = track.clone();
            m["kind"] = json!("track");
            m["name"] = track["title"].clone();
            Some(m)
        }
        Lead::Artist { name } => {
            let found = catalog::search(pandora, name).ok()?;
            let artist = found["artists"].as_array()?.iter().find(|x| same(x["name"].as_str().unwrap_or_default(), name))?;
            let mut m = artist.clone();
            m["kind"] = json!("artist");
            Some(m)
        }
        Lead::Headline { text, body } => {
            // Pandora's search matches names by prefix, so the headline's
            // opening words find the artist; whole headlines find nothing.
            // Longest first, so a pun on a longer name is seen before the
            // shorter one it starts with.
            let opening: Vec<&str> = text.split_whitespace().take(4).collect();
            let mut found = Vec::new();
            for n in (1..=opening.len()).rev() {
                if let Ok(result) = catalog::search(pandora, &opening[..n].join(" ")) {
                    found.extend(result["artists"].as_array().cloned().unwrap_or_default());
                }
                if let Some(artist) = headline_artist(text, body, &found) {
                    let mut m = artist;
                    m["kind"] = json!("artist");
                    return Some(m);
                }
            }
            None
        }
    }
}

// ---- the shared state behind the `discover` command

#[derive(Default)]
struct State {
    articles: Vec<Article>,
    fetched: HashMap<String, Fetched>,
    fetched_at: Option<Instant>,
    errors: Vec<String>,
    matches: HashMap<String, Vec<Value>>,
    loaded_cache: bool,
    resolving: bool,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn load_cache(state: &mut State) {
    if state.loaded_cache {
        return;
    }
    state.loaded_cache = true;
    if let Some(Value::Object(map)) = std::fs::read_to_string(cache_path()).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        for (k, v) in map {
            // Only hits; misses get another look each time the engine starts.
            if let Some(found) = v.as_array().filter(|a| !a.is_empty()) {
                state.matches.insert(k, found.clone());
            }
        }
    }
}

fn save_cache(state: &State) {
    // Keep only articles still in the feeds, so the file stays small.
    let live: Map<String, Value> = state
        .articles
        .iter()
        .filter_map(|a| state.matches.get(&a.id).map(|m| (a.id.clone(), json!(m))))
        .collect();
    let path = cache_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, Value::Object(live).to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn refresh(force: bool) {
    {
        let state = STATE.lock().unwrap();
        if !force && state.fetched_at.is_some_and(|t| t.elapsed() < REFRESH) {
            return;
        }
    }
    let http = match reqwest::blocking::Client::builder()
        .user_agent(format!("{USER_AGENT} RSS reader (+https://github.com/scttymn/omadora)"))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(h) => h,
        Err(_) => return,
    };
    let last = STATE.lock().unwrap().fetched.clone();
    // Every feed at once; a slow magazine should not hold up the rest.
    let results: Vec<(Feed, anyhow::Result<Fetched>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = feeds()
            .into_iter()
            .map(|feed| {
                let (http, last) = (&http, &last);
                scope.spawn(move || {
                    let got = read_feed(http, &feed, last.get(&feed.url));
                    (feed, got)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let mut articles = Vec::new();
    let mut errors = Vec::new();
    let mut fetched = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for (feed, got) in results {
        // A feed that fails this time keeps what it had last time.
        let got = got.map_err(|e| errors.push(format!("{}: {e:#}", feed.name))).ok().or_else(|| last.get(&feed.url).cloned());
        if let Some(got) = got {
            for a in &got.articles {
                    // The same review can be in two Pitchfork feeds; keep it once,
                    // under the first (most specific) feed it appeared in.
                if seen.insert(a.link.clone()) {
                    articles.push(a.clone());
                }
            }
            fetched.insert(feed.url.clone(), got);
        }
    }
    articles.sort_by_key(|a| std::cmp::Reverse(a.published));
    let mut state = STATE.lock().unwrap();
    state.articles = articles;
    state.fetched = fetched;
    state.errors = errors;
    state.fetched_at = Some(Instant::now());
}

/// Look up articles not yet matched, newest first, saving as it goes.
fn resolve_pending(pandora: Arc<Pandora>) {
    loop {
        let next = {
            let state = STATE.lock().unwrap();
            state.articles.iter().find(|a| !state.matches.contains_key(&a.id)).cloned()
        };
        let Some(article) = next else { break };
        let found: Vec<Value> = leads(&article).iter().filter_map(|lead| resolve(&pandora, lead)).collect();
        let mut state = STATE.lock().unwrap();
        state.matches.insert(article.id.clone(), found);
        save_cache(&state);
    }
    STATE.lock().unwrap().resolving = false;
}

/// The articles for the widget: newest first, with whatever is matched so
/// far. Matching continues in the background; `pending` says how many are
/// still to look up.
pub fn snapshot(pandora: Arc<Pandora>, force: bool) -> Value {
    {
        let mut state = STATE.lock().unwrap();
        load_cache(&mut state);
    }
    refresh(force);
    let mut state = STATE.lock().unwrap();
    let pending = state.articles.iter().filter(|a| !state.matches.contains_key(&a.id)).count();
    if pending > 0 && !state.resolving {
        state.resolving = true;
        std::thread::spawn(move || resolve_pending(pandora));
    }
    let mut counts: Vec<(String, usize)> = feeds().into_iter().map(|f| (f.name, 0)).collect();
    for a in &state.articles {
        if let Some(c) = counts.iter_mut().find(|(n, _)| *n == a.feed) {
            c.1 += 1;
        }
    }
    let items: Vec<Value> = state
        .articles
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "feed": a.feed,
                "title": a.title,
                "summary": a.summary,
                "link": a.link,
                "published": a.published,
                "image": a.image,
                "matches": state.matches.get(&a.id).cloned().unwrap_or_default(),
                "resolved": state.matches.contains_key(&a.id),
            })
        })
        .collect();
    json!({
        "feeds": counts.iter().map(|(n, c)| json!({ "name": n, "count": c })).collect::<Vec<_>>(),
        "items": items,
        "pending": pending,
        "errors": state.errors,
    })
}

// ---- pictures, cached on disk

/// Pictures not shown for this long are deleted.
const IMAGE_KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const IMAGE_MAX_BYTES: u64 = 8 << 20;
/// The whole picture cache, before the least recently shown go.
const IMAGE_CACHE_MAX: u64 = 200 << 20;
/// A feed this big isn't a feed.
const FEED_MAX_BYTES: u64 = 5 << 20;
/// Downloads at once; a screenful of new covers should not hit a magazine
/// all together.
const IMAGE_DOWNLOADS: usize = 4;

fn images_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".cache"));
    base.join("omadora").join("images")
}

/// A file name for a picture's URL: its SHA-256, so no crafted URL can share
/// another's file, plus the URL's extension.
fn image_file(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(url.as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect();
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| ["jpg", "jpeg", "png", "webp", "gif"].contains(&e.as_str()))
        .unwrap_or_else(|| "img".into());
    format!("{hash}.{ext}")
}

/// Delete pictures nobody has looked at in a month, then the least recently
/// shown until the cache is under its size cap.
fn prune_images(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified.elapsed().is_ok_and(|age| age > IMAGE_KEEP) {
            let _ = std::fs::remove_file(entry.path());
        } else {
            kept.push((modified, meta.len(), entry.path()));
        }
    }
    kept.sort();
    let mut total: u64 = kept.iter().map(|(_, len, _)| len).sum();
    for (_, len, path) in kept {
        if total <= IMAGE_CACHE_MAX {
            break;
        }
        let _ = std::fs::remove_file(path);
        total -= len;
    }
}

/// A web address: http or https, nothing a desktop would hand to some
/// other program.
pub fn web_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
}

/// A host on the internet, not this machine or the local network: a feed
/// shouldn't be able to point the engine at a router or a local service.
/// (Names are taken at their word; this doesn't resolve them.)
fn public_host(url: &str) -> bool {
    use std::net::IpAddr;
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    match u.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.to_ascii_lowercase();
            !(name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local") || name.ends_with(".internal"))
        }
        Some(url::Host::Ipv4(ip)) => {
            let ip = IpAddr::V4(ip);
            !(ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || matches!(ip, IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() || v4.is_broadcast() || v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64))
        }
        Some(url::Host::Ipv6(ip)) => {
            let seg = ip.segments()[0];
            !(ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80)
        }
        None => false,
    }
}

/// A response body, refused once it passes `max` bytes rather than read
/// whole first.
fn read_capped(response: reqwest::blocking::Response, max: u64) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    if response.content_length().is_some_and(|len| len > max) {
        anyhow::bail!("too large");
    }
    let mut body = Vec::new();
    response.take(max + 1).read_to_end(&mut body)?;
    if body.len() as u64 > max {
        anyhow::bail!("too large");
    }
    Ok(body)
}

static DOWNLOADS: LazyLock<(Mutex<usize>, Condvar)> = LazyLock::new(|| (Mutex::new(0), Condvar::new()));

/// A Discover article's picture as a local file, downloading it the first
/// time. Only pictures of the articles on hand are fetched.
pub fn image(url: &str) -> anyhow::Result<PathBuf> {
    let known = STATE.lock().unwrap().articles.iter().any(|a| a.image == url);
    if !known || !web_url(url) || !public_host(url) {
        anyhow::bail!("not a Discover picture");
    }
    let dir = images_dir();
    // Tidy the cache at start and every few hours after.
    static PRUNED: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = PRUNED.lock().unwrap();
        if last.is_none_or(|at| at.elapsed() > Duration::from_secs(6 * 60 * 60)) {
            *last = Some(Instant::now());
            prune_images(&dir);
        }
    }
    let path = dir.join(image_file(url));
    let touch = |path: &PathBuf| {
        if let Ok(file) = std::fs::File::options().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
    };
    if path.exists() {
        touch(&path);
        return Ok(path);
    }
    let (count, freed) = &*DOWNLOADS;
    {
        let mut running = freed.wait_while(count.lock().unwrap(), |n| *n >= IMAGE_DOWNLOADS).unwrap();
        *running += 1;
    }
    let result = (|| {
        // Another request may have fetched it while this one waited.
        if path.exists() {
            return Ok(path.clone());
        }
        let response = IMAGE_HTTP.get(url).send().map_err(reqwest::Error::without_url)?.error_for_status()?;
        let is_image = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("image/"));
        if !is_image {
            anyhow::bail!("not a picture");
        }
        let bytes = read_capped(response, IMAGE_MAX_BYTES).map_err(|_| anyhow::anyhow!("picture too large"))?;
        std::fs::create_dir_all(&dir)?;
        let tmp = path.with_extension(format!("{}.part", std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path.clone())
    })();
    *count.lock().unwrap() -= 1;
    freed.notify_one();
    result
}

static IMAGE_HTTP: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    // Redirects only to other public web addresses.
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let next = attempt.url().as_str();
        if attempt.previous().len() >= 5 || !web_url(next) || !public_host(next) {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    reqwest::blocking::Client::builder()
        .user_agent(format!("{USER_AGENT} RSS reader (+https://github.com/scttymn/omadora)"))
        .timeout(Duration::from_secs(20))
        .redirect(policy)
        .build()
        .unwrap_or_default()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitchfork_links_name_the_artist() {
        assert_eq!(
            pitchfork_artist("https://pitchfork.com/reviews/albums/rachel-chinouriri-i-think-i-spoke-too-soon/", "I think I spoke too soon"),
            "rachel chinouriri"
        );
        assert_eq!(pitchfork_artist("https://pitchfork.com/reviews/tracks/zsela-kinda-guy/", "Kinda Guy"), "zsela");
        assert_eq!(pitchfork_artist("https://pitchfork.com/reviews/albums/fontaines-dc-dopamine-chamber/", "Dopamine Chamber"), "fontaines dc");
    }

    #[test]
    fn titles_lose_quotes_and_features() {
        assert_eq!(clean_title("“Lies” [ft. Lana Del Rey]"), "Lies");
        assert_eq!(clean_title("“expectations”"), "expectations");
    }

    #[test]
    fn picture_files_are_stable_and_keep_their_type() {
        let a = image_file("https://media.pitchfork.com/photos/1/master/w_640,c_limit/A.jpg");
        assert_eq!(a, image_file("https://media.pitchfork.com/photos/1/master/w_640,c_limit/A.jpg"));
        assert!(a.ends_with(".jpg"));
        assert_ne!(a, image_file("https://media.pitchfork.com/photos/2/master/w_640,c_limit/A.jpg"));
        assert!(image_file("https://example.com/cover?size=640").ends_with(".img"));
    }

    #[test]
    fn only_public_web_addresses() {
        assert!(web_url("https://pitchfork.com/reviews/albums/x/"));
        assert!(!web_url("file:///home/me/x.desktop"));
        assert!(!web_url("steam://run/1"));
        assert!(!web_url("javascript:alert(1)"));
        assert!(public_host("https://media.pitchfork.com/a.jpg"));
        for local in ["http://localhost/a.jpg", "http://127.0.0.1/a.jpg", "http://192.168.1.1/a.jpg", "http://10.0.0.2/a.jpg", "http://169.254.169.254/x", "http://[::1]/a.jpg", "http://printer.local/a.jpg"] {
            assert!(!public_host(local), "{local}");
        }
        assert_ne!(image_file("https://a.example/x.jpg"), image_file("https://a.example/y.jpg"));
        assert!(image_file("https://a.example/x.jpg").ends_with(".jpg"));
    }

    #[test]
    fn originals_beat_remixes() {
        let found = vec![
            json!({ "title": "Souvenir (Moby Remix)", "artist": "OMD" }),
            json!({ "title": "Souvenir (Remastered 2019)", "artist": "OMD" }),
        ];
        assert_eq!(best(&found, "title", "Souvenir", "OMD").unwrap()["title"], "Souvenir (Remastered 2019)");
        let only_remix = vec![json!({ "title": "Souvenir (Moby Remix)", "artist": "OMD" })];
        assert_eq!(best(&only_remix, "title", "Souvenir", "OMD").unwrap()["title"], "Souvenir (Moby Remix)");
        assert!(harmless("(feat. Lana Del Rey)"));
        assert!(harmless("(2001 Remaster)"));
        assert!(!harmless("(Blue Silver)"));
    }

    #[test]
    fn pandora_titles_may_add_credits_and_editions() {
        assert!(extras("Lies (feat. Lana Del Rey)", "Lies").is_some());
        assert!(extras("Dopamine Chamber [Deluxe] (Remastered)", "Dopamine Chamber").is_some());
        assert_eq!(extras("Kinda Guy", "Kinda Guy"), Some(vec![]));
        assert!(extras("Lies (feat. Lana Del Rey)", "Truth").is_none());
    }

    #[test]
    fn headlines_name_the_artist_they_start_with() {
        let artists = |names: &[&str]| names.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>();
        let found = headline_artist(
            "ORBITAL REVISITS A HISTORIC MOMENT",
            "Electronic pioneers Orbital recall one of the most influential performances",
            &artists(&["Orbital", "ORBITAL 365"]),
        );
        assert_eq!(found.unwrap()["name"], "Orbital");
        let found = headline_artist(
            "The Crystal Method Celebrates 25 Years",
            "Electronic pioneers The Crystal Method commemorate",
            &artists(&["The Crystals", "The Crystal Method", "Crystal Waters"]),
        );
        assert_eq!(found.unwrap()["name"], "The Crystal Method");
        // A pun on a longer name is not about the shorter one.
        let pun = headline_artist(
            "Rage Against the Rockstars",
            "Rage, rage everywhere",
            &artists(&["Rage Against The Machine", "Prophets of Rage", "Rage"]),
        );
        assert!(pun.is_none());
        // Named in the headline but not in the article: not about them.
        assert!(headline_artist("Props and Opps for October", "Awards and openings", &artists(&["Props"])).is_none());
        // Mentioned only as a shouted section label, or as a plain word.
        assert!(headline_artist("PROPS AND OPPS", "PROPS On October 10, the awards", &artists(&["Props"])).is_none());
        assert!(headline_artist("Film Scores Tonight", "the art of film composing", &artists(&["Film"])).is_none());
    }

    #[test]
    fn artist_credits_agree_both_ways() {
        assert!(artist_agrees("Olivia Rodrigo & Chappell Roan", "olivia rodrigo chappell roan"));
        assert!(artist_agrees("Fontaines D.C.", "fontaines dc"));
        assert!(artist_agrees("Rachel Chinouriri", "rachel chinouriri"));
        // A mix series shared by many artists must not match the wrong one.
        assert!(!artist_agrees("Henrik Schwarz", "djrum"));
    }

    #[test]
    fn tags_skip_sections_and_favor_the_headline() {
        let a = Article {
            id: "1".into(),
            feed: "Rolling Stone".into(),
            matcher: Matcher::Tags,
            title: "See Red Hot Chili Peppers, Dave Grohl Perform at Silverlake Conservatory".into(),
            summary: String::new(),
            link: "https://example.com/x".into(),
            published: 0,
            image: String::new(),
            categories: ["Live Reviews", "Music", "Anthony Kiedis", "Dave Grohl", "Flea", "Foo Fighters", "Red Hot Chili Peppers"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        assert_eq!(artist_tags(&a), vec!["Dave Grohl", "Red Hot Chili Peppers", "Anthony Kiedis", "Flea"]);
    }

    #[test]
    fn title_style_splits_artist_and_album() {
        let a = Article {
            id: "1".into(),
            feed: "f".into(),
            matcher: Matcher::Title,
            title: "The Avalanches – No Bad Memories".into(),
            summary: String::new(),
            link: "https://example.com/x".into(),
            published: 0,
            image: String::new(),
            categories: Vec::new(),
        };
        assert_eq!(leads(&a), vec![Lead::Album { title: "No Bad Memories".into(), artist: "The Avalanches".into() }]);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn thumbnails_come_from_the_xml() {
        let xml = r#"<rss><channel><item><title>A</title><link>https://pitchfork.com/reviews/albums/x-a/</link>
            <media:thumbnail url="https://media.pitchfork.com/photos/1/master/pass/A.jpg" width="2500"/></item></channel></rss>"#;
        let images = raw_images(xml);
        let url = images.get("https://pitchfork.com/reviews/albums/x-a/").cloned().unwrap();
        assert_eq!(card_size(url), "https://media.pitchfork.com/photos/1/master/w_640,c_limit/A.jpg");
    }
}
