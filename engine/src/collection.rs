//! The listener's collection ("My Collection"): the albums, songs, and
//! playlists they've added. Liking a song outside a station adds it here,
//! and the Artists list is built from it, as in Pandora's own apps.

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
    let collected: Vec<String> = items(pandora)?
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
        if !seen.insert(id.clone()) {
            continue;
        }
        let mine = by_artist.entry(artist.to_string()).or_default();
        if mine.name.is_empty() {
            mine.name = item["artistName"].as_str().unwrap_or_default().to_string();
        }
        if id.starts_with("AL:") {
            mine.albums.push(catalog::album(item));
        } else {
            mine.songs.push(catalog::track(item));
        }
    }
    let index = Arc::new(by_artist);
    *INDEX.lock().unwrap() = Some((Instant::now(), index.clone()));
    Ok(index)
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
