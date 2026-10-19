//! The listener's collection ("My Collection"): the albums, songs, and
//! playlists they've added. Liking a song outside a station adds it here,
//! a station thumb lands in its My Thumbs Up playlist, and the Artists list
//! is built from both, as in Pandora's own apps.

use crate::catalog;
use crate::pandora::Pandora;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Everything in the collection, page by page.
pub fn items(pandora: &Pandora) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    let mut cursor = String::new();
    // A collection is a few hundred items; this is only a stop for a cursor
    // that never ends.
    for _ in 0..100 {
        let body = if cursor.is_empty() { json!({}) } else { json!({ "cursor": cursor }) };
        let page = pandora.call("v4/collections/getItems", body)?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        let next = page["cursor"].as_str().unwrap_or_default().to_string();
        let done = items.is_empty() || next.is_empty() || next == cursor;
        all.extend(items);
        if done {
            break;
        }
        cursor = next;
    }
    Ok(all)
}

/// The ids of everything collected, to show whether a song is.
pub fn ids(pandora: &Pandora) -> Result<HashSet<String>> {
    Ok(items(pandora)?.iter().filter_map(|i| i["pandoraId"].as_str().map(str::to_string)).collect())
}

/// Add an album, song, or playlist to the collection, or take it out.
pub fn set(pandora: &Pandora, id: &str, add: bool) -> Result<()> {
    if !(id.starts_with("TR:") || id.starts_with("AL:") || id.starts_with("PL:")) {
        bail!("Only songs, albums, and playlists can be collected.");
    }
    let path = if add { "v4/collections/addItem" } else { "v4/collections/removeItem" };
    pandora.call(path, json!({ "pandoraId": id }))?;
    if add {
        REMOVED.lock().unwrap().retain(|(_, r)| r != id);
    }
    *INDEX.lock().unwrap() = None;
    Ok(())
}

/// One artist's share of the collection: their name, and the albums and
/// songs of theirs that are collected, shaped like the catalog's.
#[derive(Clone, Default)]
struct Mine {
    name: String,
    albums: Vec<Value>,
    songs: Vec<Value>,
}

/// The collection by artist, kept for a minute (and dropped when something
/// is collected or taken out): the Artists tab and every artist page read it.
static INDEX: Mutex<Option<(Instant, Arc<BTreeMap<String, Mine>>)>> = Mutex::new(None);

fn index(pandora: &Pandora) -> Result<Arc<BTreeMap<String, Mine>>> {
    if let Some((at, index)) = INDEX.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return Ok(index.clone());
        }
    }
    let all = items(pandora)?;
    let collected: Vec<String> = all
        .iter()
        .filter(|i| matches!(i["pandoraType"].as_str(), Some("AL" | "TR")))
        .filter_map(|i| i["pandoraId"].as_str().map(str::to_string))
        .collect();
    let details = catalog::annotate(pandora, &collected)?;
    let mut by_artist: BTreeMap<String, Mine> = BTreeMap::new();
    let mut seen = HashSet::new();
    for id in &collected {
        let Some(item) = details.get(id) else { continue };
        let Some(artist) = item["artistId"].as_str().filter(|a| !a.is_empty()) else { continue };
        if removed_lately(id) {
            continue;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        let mine = by_artist.entry(artist.to_string()).or_default();
        if mine.name.is_empty() {
            mine.name = item["artistName"].as_str().unwrap_or_default().to_string();
        }
        let mut shaped = if id.starts_with("AL:") { catalog::album(item) } else { catalog::track(item) };
        shaped["from"] = json!(["collection"]);
        if id.starts_with("AL:") {
            mine.albums.push(shaped);
        } else {
            mine.songs.push(shaped);
        }
    }
    // Songs thumbed up on stations: Pandora keeps them in the collection's
    // My Thumbs Up playlist, on every plan.
    let thumbs: Vec<Value> = all
        .iter()
        .filter(|i| i["linkedType"] == "MyThumbsUp")
        .filter_map(|i| i["pandoraId"].as_str())
        .filter_map(|id| catalog::playlist_page(pandora, id).ok())
        .flat_map(|page| page["tracks"].as_array().cloned().unwrap_or_default())
        .collect();
    for mut track in thumbs {
        let (Some(id), Some(artist)) = (track["id"].as_str().map(str::to_string), track["artistId"].as_str().filter(|a| !a.is_empty()).map(str::to_string)) else {
            continue;
        };
        if removed_lately(&id) {
            continue;
        }
        let mine = by_artist.entry(artist).or_default();
        // Collected and thumbed up both: one entry, from both.
        if !seen.insert(id.clone()) {
            if let Some(song) = mine.songs.iter_mut().find(|s| s["id"] == id.as_str()) {
                song["from"].as_array_mut().map(|f| f.push(json!("thumbs")));
            }
            continue;
        }
        if mine.name.is_empty() {
            mine.name = track["artist"].as_str().unwrap_or_default().to_string();
        }
        track["from"] = json!(["thumbs"]);
        mine.songs.push(track);
    }
    // Thumbs Pandora hasn't put in My Thumbs Up yet.
    let pending: Vec<Value> = PENDING
        .lock()
        .unwrap()
        .iter()
        .filter(|(at, _)| at.elapsed() < PENDING_FOR)
        .map(|(_, t)| t.clone())
        .collect();
    for mut track in pending {
        let (Some(id), Some(artist)) = (track["id"].as_str().map(str::to_string), track["artistId"].as_str().filter(|a| !a.is_empty()).map(str::to_string)) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let mine = by_artist.entry(artist).or_default();
        if mine.name.is_empty() {
            mine.name = track["artist"].as_str().unwrap_or_default().to_string();
        }
        track["from"] = json!(["thumbs"]);
        mine.songs.push(track);
    }
    let index = Arc::new(by_artist);
    *INDEX.lock().unwrap() = Some((Instant::now(), index.clone()));
    Ok(index)
}

/// Take a song or album out: out of the collection, and every station
/// thumb up on it removed, as the listener asked. Thumbs are found among
/// the stations' feedback (the My Thumbs Up playlist doesn't say which
/// station), a few stations at a time.
pub fn remove(pandora: &Pandora, id: &str, from: &[String]) -> Result<()> {
    forget_pending(id);
    REMOVED.lock().unwrap().push((Instant::now(), id.to_string()));
    if from.iter().any(|f| f == "collection") {
        set(pandora, id, false)?;
    }
    if from.iter().any(|f| f == "thumbs") {
        remove_thumbs(pandora, id)?;
    }
    changed();
    Ok(())
}

fn remove_thumbs(pandora: &Pandora, id: &str) -> Result<()> {
    let list = pandora.device_call("user.getStationList", json!({}))?;
    let stations: Vec<String> = list["stations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| !s["isQuickMix"].as_bool().unwrap_or(false))
        .filter_map(|s| s["stationToken"].as_str().map(str::to_string))
        .collect();
    let feedback: Vec<String> = stations
        .chunks(8)
        .flat_map(|chunk| {
            std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|station| {
                        scope.spawn(move || {
                            pandora
                                .device_call("station.getStation", json!({ "stationToken": station, "includeExtendedAttributes": true }))
                                .map(|st| {
                                    st["feedback"]["thumbsUp"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .filter(|t| t["pandoraId"] == id)
                                        .filter_map(|t| t["feedbackId"].as_str().map(str::to_string))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        })
                    })
                    .collect();
                handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect::<Vec<_>>()
            })
        })
        .collect();
    for f in &feedback {
        pandora.device_call("station.deleteFeedback", json!({ "feedbackId": f }))?;
    }
    // A thumb whose station is gone stays in My Thumbs Up; take it out there.
    let listed = remove_from_thumbs_playlist(pandora, id)?;
    if feedback.is_empty() && !listed {
        bail!("Couldn't find that thumb up on any station or in My Thumbs Up.");
    }
    Ok(())
}

/// Take a song out of the My Thumbs Up playlist; whether it was there.
fn remove_from_thumbs_playlist(pandora: &Pandora, id: &str) -> Result<bool> {
    let Some(playlist) = items(pandora)?
        .into_iter()
        .find(|i| i["linkedType"] == "MyThumbsUp")
        .and_then(|i| i["pandoraId"].as_str().map(str::to_string))
    else {
        return Ok(false);
    };
    let mut version = 0u64;
    let mut offset = 0u64;
    let mut item_ids: Vec<Value> = Vec::new();
    loop {
        let page = pandora.call(
            "v7/playlists/getTracks",
            json!({ "request": {
                "pandoraId": playlist, "playlistVersion": version, "offset": offset, "limit": 100,
                "annotationLimit": 0, "allowedTypes": ["TR", "AM"], "bypassPrivacyRules": true,
            }}),
        )?;
        if offset == 0 {
            version = page["version"].as_u64().unwrap_or(0);
        }
        let tracks = page["tracks"].as_array().cloned().unwrap_or_default();
        item_ids.extend(
            tracks
                .iter()
                .filter(|t| t["pandoraId"] == id || t["trackPandoraId"] == id)
                .map(|t| t["itemId"].clone()),
        );
        offset += tracks.len() as u64;
        if tracks.is_empty() || offset >= page["totalTracks"].as_u64().unwrap_or(0) {
            break;
        }
    }
    if item_ids.is_empty() {
        return Ok(false);
    }
    pandora.call(
        "v7/playlists/deleteTracks",
        json!({ "request": { "pandoraId": playlist, "playlistVersion": version, "trackItemIds": item_ids } }),
    )?;
    Ok(true)
}

/// Songs thumbed up a moment ago: Pandora adds them to My Thumbs Up a
/// little later, so they count from the thumb on, until the playlist has
/// them (or a few minutes pass, or they're thumbed down or taken out).
static PENDING: Mutex<Vec<(Instant, Value)>> = Mutex::new(Vec::new());
const PENDING_FOR: Duration = Duration::from_secs(5 * 60);

/// A song was thumbed up (or down) on a station.
pub fn thumbed(track: Value, up: bool) {
    let id = track["id"].as_str().unwrap_or_default().to_string();
    if id.is_empty() {
        return;
    }
    if up {
        REMOVED.lock().unwrap().retain(|(_, r)| *r != id);
    }
    let mut pending = PENDING.lock().unwrap();
    pending.retain(|(at, t)| t["id"] != id.as_str() && at.elapsed() < PENDING_FOR);
    if up {
        pending.push((Instant::now(), track));
    }
    drop(pending);
    changed();
}

fn forget_pending(id: &str) {
    PENDING.lock().unwrap().retain(|(_, t)| t["id"] != id);
}

/// Songs and albums taken out a moment ago: Pandora can still list them
/// for a little while, so they stay out until it catches up.
static REMOVED: Mutex<Vec<(Instant, String)>> = Mutex::new(Vec::new());

fn removed_lately(id: &str) -> bool {
    let mut removed = REMOVED.lock().unwrap();
    removed.retain(|(at, _)| at.elapsed() < PENDING_FOR);
    removed.iter().any(|(_, r)| r == id)
}

/// Something was collected, liked, or taken out: read the collection again
/// next time.
pub fn changed() {
    *INDEX.lock().unwrap() = None;
}

/// The artists behind the collected albums and songs, by name, with how much
/// of each is collected. Pandora's collection holds no artists itself.
pub fn artists(pandora: &Pandora) -> Result<Value> {
    let index = index(pandora)?;
    let ids: Vec<String> = index.keys().cloned().collect();
    let artist_details = catalog::annotate(pandora, &ids)?;
    let mut list: Vec<Value> = index
        .iter()
        .map(|(id, mine)| {
            let mut shaped = artist_details.get(id).map(catalog::artist).unwrap_or_else(|| json!({ "id": id, "name": mine.name }));
            if shaped["name"].as_str().unwrap_or_default().is_empty() {
                shaped["name"] = json!(mine.name);
            }
            shaped["albums"] = json!(mine.albums.len());
            shaped["songs"] = json!(mine.songs.len());
            shaped
        })
        .collect();
    list.sort_by_key(|a| a["name"].as_str().unwrap_or_default().to_lowercase());
    Ok(json!(list))
}

/// What of one artist is collected, for their page.
pub fn for_artist(pandora: &Pandora, artist: &str) -> Result<Value> {
    let index = index(pandora)?;
    let mine = index.get(artist).cloned().unwrap_or_default();
    Ok(json!({ "albums": mine.albums, "songs": mine.songs }))
}
