//! Catalog lookups, flattened into the shapes the widget reads.
//!
//! Pandora answers with IDs plus an `annotations` map of details. Everything
//! here resolves those into plain objects: artist, album, track, playlist.

use crate::pandora::Pandora;
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const ART_HOST: &str = "https://content-images.p-cdn.com/";

/// Art at a given size from an annotation's `icon`.
fn art(icon: &Value, size: u32) -> String {
    match icon["artId"].as_str() {
        Some(id) if !id.is_empty() => format!("{ART_HOST}{id}_{size}W_{size}H.jpg"),
        _ => String::new(),
    }
}

/// Playlist covers are compositing recipes ("thor layers"); Pandora's image
/// service renders them the way its apps show them.
pub(crate) fn thor(layers: &Value, size: u32) -> String {
    match layers.as_str() {
        Some(l) if !l.is_empty() => {
            format!("https://dyn-images.p-cdn.com/?l={}&w={size}&h={size}", urlencoding::encode(&steady_grids(l)))
        }
        _ => String::new(),
    }
}

/// Pandora lists some playlists' covers in a new order on every fetch, which
/// made a new collage (a new download, the covers rearranged) each time the
/// page was read. Sorted, the same covers make the same picture.
fn steady_grids(layers: &str) -> String {
    let mut out = String::with_capacity(layers.len());
    let mut rest = layers;
    while let Some(start) = rest.find("grid(") {
        let open = start + "grid(".len();
        let Some(len) = rest[open..].find(')') else { break };
        let mut cells: Vec<&str> = rest[open..open + len].split(',').collect();
        cells.sort_unstable();
        out.push_str(&rest[..open]);
        out.push_str(&cells.join(","));
        rest = &rest[open + len..];
    }
    out.push_str(rest);
    out
}

fn color(icon: &Value) -> String {
    icon["dominantColor"].as_str().map(|c| format!("#{c}")).unwrap_or_default()
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// Pandora's bios are HTML with styling spans and links to other artists.
/// Keep only the links, as `<a href="AR:…">`, so the widget can render the
/// bio as rich text and open those artists; drop every other tag.
fn clean_bio(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&escape(&rest[..start]));
        let Some(end) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = &rest[start..start + end + 1];
        if tag.starts_with("<a ") {
            let id = tag
                .split("data-pandora-id=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .filter(|id| id.starts_with("AR:") || id.starts_with("AL:") || id.starts_with("TR:"));
            match id {
                Some(id) => out.push_str(&format!("<a href=\"{id}\">")),
                None => out.push_str("<a>"),
            }
        } else if tag == "</a>" {
            out.push_str("</a>");
        } else if tag.starts_with("<br") || tag == "<p>" || tag == "</p>" {
            out.push_str("<br>");
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(&escape(rest));
    out.trim().replace("\n\n", "<br><br>")
}

/// Escape a bare `&` for rich text; leave entities such as `&amp;` alone.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, ch) in text.char_indices() {
        let entity = ch == '&'
            && text[i + 1..]
                .find(';')
                .is_some_and(|end| end > 0 && end <= 8 && text[i + 1..i + 1 + end].chars().all(|c| c.is_ascii_alphanumeric() || c == '#'));
        // Angle brackets too: a stray "<img…" at the end of a bio must not
        // reach the rich-text label as a tag.
        match ch {
            '&' if !entity => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// On-demand play needs interactive rights on the item.
fn playable(a: &Value) -> bool {
    a["rightsInfo"]["hasInteractive"].as_bool().unwrap_or(false)
}

pub fn track(a: &Value) -> Value {
    json!({
        "id": a["pandoraId"],
        "title": text(&a["name"]),
        "artist": text(&a["artistName"]),
        "artistId": a["artistId"],
        "album": text(&a["albumName"]),
        "albumId": a["albumId"],
        "number": a["trackNumber"],
        "duration": a["duration"],
        "explicit": a["explicitness"] == "EXPLICIT",
        "art": art(&a["icon"], 500),
        "color": color(&a["icon"]),
        "playable": playable(a),
    })
}

pub fn album(a: &Value) -> Value {
    json!({
        "id": a["pandoraId"],
        "name": text(&a["name"]),
        "artist": text(&a["artistName"]),
        "artistId": a["artistId"],
        "year": text(&a["releaseDate"]).get(..4).unwrap_or_default(),
        "type": text(&a["releaseType"]),
        "trackCount": a["trackCount"],
        "art": art(&a["icon"], 500),
        "color": color(&a["icon"]),
        "playable": playable(a),
    })
}

pub fn artist(a: &Value) -> Value {
    json!({
        "id": a["pandoraId"],
        "name": text(&a["name"]),
        "art": art(&a["icon"], 500),
        "color": color(&a["icon"]),
        "albumCount": a["albumCount"],
        "trackCount": a["trackCount"],
    })
}

/// Pandora's own mixes get a collage of different covers, picked from many
/// songs, on every fetch. Keep the first one shown for each playlist while
/// the engine runs, so tiles don't change under the listener on every
/// refresh; it's new again after a restart.
pub(crate) fn steady_art(id: &Value, art: String) -> String {
    static SHOWN: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
    match id.as_str() {
        Some(id) if !id.is_empty() && !art.is_empty() => SHOWN.lock().unwrap().entry(id.to_string()).or_insert(art).clone(),
        _ => art,
    }
}

pub fn playlist(a: &Value) -> Value {
    json!({
        "id": a["pandoraId"],
        "name": text(&a["name"]),
        "trackCount": a["totalTracks"],
        "art": steady_art(&a["pandoraId"], thor(&a["thorLayers"], 500)),
        "color": color(&a["icon"]),
        "owned": a["viewerInfo"]["editable"].as_bool().unwrap_or(false),
    })
}

/// Shape an annotation by its type.
fn shape(a: &Value) -> Value {
    match a["type"].as_str() {
        Some("TR") => track(a),
        Some("AL") => album(a),
        Some("AR") => artist(a),
        Some("PL") => playlist(a),
        _ => a.clone(),
    }
}

/// Details for many IDs, in one round trip per 100.
pub fn annotate(pandora: &Pandora, ids: &[String]) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for chunk in ids.chunks(100) {
        let found = pandora.call("v4/catalog/annotateObjects", json!({ "pandoraIds": chunk }))?;
        if let Value::Object(map) = found {
            out.extend(map);
        }
    }
    Ok(out)
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn pick(annotations: &Map<String, Value>, wanted: &[String]) -> Vec<Value> {
    wanted.iter().filter_map(|id| annotations.get(id)).map(shape).collect()
}

pub fn search(pandora: &Pandora, query: &str) -> Result<Value> {
    let found = pandora.call(
        "v3/sod/search",
        json!({
            "query": query,
            "types": ["AR", "AL", "TR", "PL"],
            "listener": null,
            "start": 0,
            "count": 40,
            "annotate": true,
            "searchTime": 0,
            "annotationRecipe": "CLASS_OF_2019",
        }),
    )?;
    let annotations = found["annotations"].as_object().cloned().unwrap_or_default();
    let mut groups: Map<String, Value> = ["artists", "albums", "tracks", "playlists"]
        .iter()
        .map(|k| (k.to_string(), json!([])))
        .collect();
    for id in ids(&found["results"]) {
        let Some(a) = annotations.get(&id) else { continue };
        let key = match a["type"].as_str() {
            Some("AR") => "artists",
            Some("AL") => "albums",
            Some("TR") => "tracks",
            Some("PL") => "playlists",
            _ => continue,
        };
        groups[key].as_array_mut().unwrap().push(shape(a));
    }
    Ok(Value::Object(groups))
}

pub fn artist_page(pandora: &Pandora, id: &str) -> Result<Value> {
    let details = pandora.call("v4/catalog/getDetails", json!({ "pandoraId": id }))?;
    let d = &details["artistDetails"];
    let discography = pandora
        .call("v4/catalog/getArtistDiscographyWithCollaborations", json!({ "artistPandoraId": id, "annotationLimit": 0 }))
        .map(|v| ids(&v["discography"]))
        .unwrap_or_default();
    let top_tracks = ids(&d["topTracks"]);
    let similar = ids(&d["similarArtists"]);
    let mut wanted = vec![id.to_string()];
    wanted.extend(discography.iter().cloned());
    wanted.extend(top_tracks.iter().cloned());
    wanted.extend(similar.iter().cloned());
    let mut annotations = details["annotations"].as_object().cloned().unwrap_or_default();
    let missing: Vec<String> = wanted.iter().filter(|w| !annotations.contains_key(*w)).cloned().collect();
    annotations.extend(annotate(pandora, &missing)?);

    let me = annotations.get(id).cloned().unwrap_or(Value::Null);
    let traits: Vec<Value> = d["focusTraits"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t["name"].as_str()).map(|s| json!(s)).collect())
        .unwrap_or_default();
    Ok(json!({
        "id": id,
        "name": text(&me["name"]),
        "bio": clean_bio(&text(&d["bio"])),
        "art": art(&me["icon"], 500),
        "image": art(&d["heroImage"], 1080),
        "color": color(&me["icon"]),
        "traits": traits,
        "latest": d["latestRelease"],
        "topTracks": pick(&annotations, &top_tracks),
        "albums": pick(&annotations, &discography),
        "similar": pick(&annotations, &similar),
    }))
}

pub fn album_page(pandora: &Pandora, id: &str) -> Result<Value> {
    let mut annotations = annotate(pandora, &[id.to_string()])?;
    let me = annotations.get(id).cloned().unwrap_or(Value::Null);
    let tracks = ids(&me["tracks"]);
    annotations.extend(annotate(pandora, &tracks)?);
    let mut page = album(&me);
    page["tracks"] = json!(pick(&annotations, &tracks));
    page["art"] = json!(art(&me["icon"], 1080));
    Ok(page)
}

// Pandora refuses to annotate more than 100 objects per request.
const PAGE: u64 = 100;

pub fn playlists(pandora: &Pandora) -> Result<Value> {
    let mut items = Vec::new();
    let mut offset = 0;
    loop {
        let found = pandora.call(
            "v7/collections/getSortedPlaylists",
            json!({ "request": {
                "sortOrder": "MOST_RECENT_MODIFIED", "offset": offset, "limit": PAGE, "annotationLimit": PAGE,
            }}),
        )?;
        let annotations = found["annotations"].as_object().cloned().unwrap_or_default();
        let page = found["items"].as_array().cloned().unwrap_or_default();
        items.extend(page.iter().filter_map(|i| i["pandoraId"].as_str()).filter_map(|id| annotations.get(id)).map(playlist));
        offset += page.len() as u64;
        if page.is_empty() || offset >= found["totalCount"].as_u64().unwrap_or(0) {
            break;
        }
    }
    Ok(json!(items))
}

pub fn playlist_page(pandora: &Pandora, id: &str) -> Result<Value> {
    let mut tracks = Vec::new();
    let mut name = String::new();
    let mut total = 0u64;
    let mut cover = Value::Null;
    let mut offset = 0u64;
    // Later pages must name the version the first page reported.
    let mut version = 0u64;
    loop {
        let page = pandora.call(
            "v7/playlists/getTracks",
            json!({ "request": {
                "pandoraId": id, "playlistVersion": version, "offset": offset, "limit": PAGE,
                "annotationLimit": PAGE, "allowedTypes": ["TR", "AM"], "bypassPrivacyRules": true,
            }}),
        )?;
        if offset == 0 {
            name = text(&page["name"]);
            total = page["totalTracks"].as_u64().unwrap_or(0);
            cover = page["thorLayers"].clone();
            version = page["version"].as_u64().unwrap_or(0);
        }
        let annotations = page["annotations"].as_object().cloned().unwrap_or_default();
        let batch: Vec<Value> = page["tracks"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t["trackPandoraId"].as_str().or(t["pandoraId"].as_str()))
                    .filter_map(|tid| annotations.get(tid))
                    .map(track)
                    .collect()
            })
            .unwrap_or_default();
        let got = page["tracks"].as_array().map(Vec::len).unwrap_or(0) as u64;
        tracks.extend(batch);
        offset += got;
        if got == 0 || offset >= total {
            break;
        }
    }
    Ok(json!({ "id": id, "name": name, "trackCount": total, "art": thor(&cover, 640), "tracks": tracks }))
}

/// The tracks behind something playable: an album, a playlist, or one track.
pub fn tracks_for(pandora: &Pandora, source: &str) -> Result<(String, Vec<Value>)> {
    match source.split(':').next() {
        Some("AL") => {
            let page = album_page(pandora, source)?;
            Ok((text(&page["name"]), page["tracks"].as_array().cloned().unwrap_or_default()))
        }
        Some("PL") => {
            let page = playlist_page(pandora, source)?;
            Ok((text(&page["name"]), page["tracks"].as_array().cloned().unwrap_or_default()))
        }
        Some("TR") => {
            let found = annotate(pandora, &[source.to_string()])?;
            let t = found.get(source).map(track).unwrap_or(Value::Null);
            Ok((text(&t["title"]), vec![t]))
        }
        _ => anyhow::bail!("cannot play {source}: expected an album, playlist, or track id"),
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_bio, steady_art, steady_grids};

    #[test]
    fn collage_covers_in_a_steady_order() {
        let a = "images/aa/:cover();grid(images/c3/@1/3,images/a1/@1/3,images/b2/@1/3);overlay($1)";
        let b = "images/aa/:cover();grid(images/b2/@1/3,images/c3/@1/3,images/a1/@1/3);overlay($1)";
        assert_eq!(steady_grids(a), "images/aa/:cover();grid(images/a1/@1/3,images/b2/@1/3,images/c3/@1/3);overlay($1)");
        assert_eq!(steady_grids(a), steady_grids(b));
        assert_eq!(steady_grids("images/aa/"), "images/aa/");
        assert_eq!(steady_grids("_;grid(images/b/,images/a/"), "_;grid(images/b/,images/a/");
    }

    #[test]
    fn a_playlist_keeps_its_first_collage() {
        let id = serde_json::json!("PL:test:1");
        assert_eq!(steady_art(&id, "first".into()), "first");
        assert_eq!(steady_art(&id, "second".into()), "first");
        assert_eq!(steady_art(&serde_json::json!("PL:test:2"), "other".into()), "other");
        assert_eq!(steady_art(&serde_json::json!(null), "loose".into()), "loose");
    }

    #[test]
    fn bio_keeps_artist_links_only() {
        let html = r#"The band<span class="pull-single">’</span>s songs &amp; <a data-pandora-id="AR:12345" href="pandorav4:/x">a friend</a>."#;
        assert_eq!(clean_bio(html), r#"The band’s songs &amp; <a href="AR:12345">a friend</a>."#);
        assert_eq!(clean_bio("R&B and soul"), "R&amp;B and soul");
    }
}
