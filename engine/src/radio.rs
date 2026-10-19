//! Radio: stations, their playlists, feedback, and modes, on the web API.
//!
//! A station plays in fragments of a few tracks each. Every track carries a
//! token that feedback, "tired", and play reports refer to, plus its own
//! rights (may it be skipped, rated, replayed). Pandora allows one listener
//! per account; a second gets STREAM_VIOLATION, surfaced as `Busy`.

use crate::catalog;
use crate::pandora::{Pandora, PandoraError};
use anyhow::Result;
use serde_json::{Value, json};

/// Another device is playing this account.
#[derive(Debug)]
pub struct Busy(pub String);

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Busy {}

/// The largest picture in one of Pandora's `art` lists, up to `max`.
fn pick_art(art: &Value, max: u64) -> String {
    let mut best = (0, String::new());
    for a in art.as_array().into_iter().flatten() {
        let size = a["size"].as_u64().unwrap_or(0);
        if size <= max && size >= best.0 {
            best = (size, a["url"].as_str().unwrap_or_default().to_string());
        }
    }
    best.1
}

fn station(s: &Value) -> Value {
    json!({
        "id": s["stationId"],
        "name": s["name"],
        "art": pick_art(&s["art"], 500),
        "thumbprint": s["isThumbprint"].as_bool().unwrap_or(false),
        "shuffle": s["isShuffle"].as_bool().unwrap_or(false),
        "lastPlayed": s["lastPlayed"],
        "seed": s["initialSeed"]["pandoraId"],
    })
}

pub fn stations(pandora: &Pandora) -> Result<Value> {
    let found = pandora.call("v1/station/getStations", json!({ "pageSize": 250 }))?;
    let mut list: Vec<Value> = found["stations"].as_array().into_iter().flatten().map(station).collect();
    list.sort_by_key(|s| s["name"].as_str().unwrap_or_default().to_lowercase());
    Ok(json!(list))
}

/// Shuffle plays across all stations; Pandora hands back a station for it.
pub fn shuffle(pandora: &Pandora) -> Result<Value> {
    let s = pandora.call("v1/station/shuffle", json!({}))?;
    let mut out = station(&s);
    out["shuffle"] = json!(true);
    Ok(out)
}

/// A new station from an artist or song id; Pandora returns an existing
/// station if the listener already has one for it.
pub fn create(pandora: &Pandora, seed: &str) -> Result<Value> {
    let s = pandora.call(
        "v1/station/createStation",
        json!({ "pandoraId": seed, "stationCode": null, "stationName": "", "searchQuery": "", "creativeId": null, "lineId": null }),
    )?;
    Ok(station(&s))
}

/// The next few tracks of a station, with album and artist ids filled in
/// from the catalog so their pages open from the player.
pub fn fragment(pandora: &Pandora, station_id: &str, starting: bool) -> Result<Vec<Value>> {
    let (status, found) = pandora.post(
        "v1/playlist/getFragment",
        &json!({
            "stationId": station_id,
            "isStationStart": starting,
            "fragmentRequestReason": "Normal",
            "audioFormat": "aacplus",
            "startingAtTrackId": null,
            "onDemandArtistMessageArtistUidHex": null,
            "onDemandArtistMessageIdHex": null,
        }),
    )?;
    if found["errorString"] == "STREAM_VIOLATION" {
        return Err(Busy(busy_message(&found)).into());
    }
    if status != 200 || found.get("errorCode").is_some() {
        return Err(PandoraError::from_body("v1/playlist/getFragment", status, &found).into());
    }
    let raw: Vec<Value> = found["tracks"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = raw
        .iter()
        .filter(|t| t["trackType"] == "Track")
        .filter_map(|t| t["pandoraId"].as_str().map(str::to_string))
        .collect();
    let annotations = catalog::annotate(pandora, &ids).unwrap_or_default();
    Ok(raw
        .iter()
        .filter(|t| t["audioURL"].as_str().is_some_and(|u| !u.is_empty()))
        .map(|t| radio_track(t, annotations.get(t["pandoraId"].as_str().unwrap_or_default())))
        .collect())
}

fn radio_track(t: &Value, known: Option<&Value>) -> Value {
    let rights: Vec<&str> = t["rights"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
    let song = t["trackType"] == "Track";
    let mut out = match known {
        Some(a) => catalog::track(a),
        None => json!({
            "id": t["pandoraId"],
            "title": t["songTitle"],
            "artist": t["artistName"],
            "album": t["albumTitle"],
            "duration": t["trackLength"],
            "art": pick_art(&t["albumArt"], 500),
        }),
    };
    // What the fragment says wins over the catalog for this play.
    out["title"] = t["songTitle"].clone();
    out["duration"] = t["trackLength"].clone();
    if out["art"].as_str().unwrap_or_default().is_empty() {
        out["art"] = json!(pick_art(&t["albumArt"], 500));
    }
    out["kind"] = json!(if song { "song" } else { "notice" });
    out["rating"] = t["rating"].clone();
    out["canRate"] = json!(song && t["allowFeedback"].as_bool().unwrap_or(false));
    out["canTire"] = json!(song && t["allowTiredOfTrack"].as_bool().unwrap_or(false));
    out["canSkip"] = json!(t["allowSkip"].as_bool().unwrap_or(false));
    out["canReplay"] = json!(rights.contains(&"allowReplay"));
    // Kept by the engine, never shown: what play reports and feedback need.
    out["token"] = t["trackToken"].clone();
    out["audioUrl"] = t["audioURL"].clone();
    out["encoding"] = t["audioEncoding"].clone();
    out["gain"] = t["fileGain"].clone();
    out["receipt"] = t["audioReceiptURL"].clone();
    out["skipUrl"] = t["audioSkipUrl"].clone();
    out
}

pub fn feedback(pandora: &Pandora, token: &str, positive: bool) -> Result<()> {
    pandora.call("v1/station/addFeedback", json!({ "trackToken": token, "isPositive": positive }))?;
    Ok(())
}

pub fn tired(pandora: &Pandora, token: &str) -> Result<()> {
    pandora.call("v1/listener/addTiredSong", json!({ "trackToken": token }))?;
    Ok(())
}

pub fn track_started(pandora: &Pandora, token: &str) {
    let _ = pandora.call("v1/station/trackStarted", json!({ "trackToken": token }));
}

/// Tell Pandora playback resumed here. With `take_over`, this device becomes
/// the account's one listener, as Pandora's "Let me listen" does; without
/// it, a conflict comes back as `Busy`.
pub fn playback_resumed(pandora: &Pandora, take_over: bool) -> Result<()> {
    let (status, found) = pandora.post("v1/station/playbackResumed", &json!({ "forceActive": take_over }))?;
    if found["errorString"] == "STREAM_VIOLATION" {
        return Err(Busy(busy_message(&found)).into());
    }
    if status != 200 || found.get("errorCode").is_some() {
        return Err(PandoraError::from_body("v1/station/playbackResumed", status, &found).into());
    }
    Ok(())
}

/// Omadora's own words, not Pandora's dialogue, which names its apps.
fn busy_message(_found: &Value) -> String {
    "Someone else is listening on your account. Only one person can listen at a time.".to_string()
}

pub fn playback_paused(pandora: &Pandora) {
    let _ = pandora.call("v1/station/playbackPaused", json!({ "sync": false }));
}

fn shape_modes(found: &Value) -> Value {
    let current = found["currentModeId"].clone();
    let modes: Vec<Value> = found["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            json!({
                "id": m["modeId"],
                "name": m["modeName"],
                "description": m["modeDescription"],
                "active": m["modeId"] == current,
            })
        })
        .collect();
    json!({ "current": current, "modes": modes })
}

pub fn modes(pandora: &Pandora, station_id: &str) -> Result<Value> {
    let found = pandora.call("v1/interactiveradio/getAvailableModesSimple", json!({ "stationId": station_id }))?;
    Ok(shape_modes(&found))
}

pub fn set_mode(pandora: &Pandora, station_id: &str, mode_id: i64) -> Result<Value> {
    let found = pandora.call(
        "v1/interactiveradio/setAndGetAvailableModes",
        json!({ "stationId": station_id, "modeId": mode_id }),
    )?;
    Ok(shape_modes(&found))
}
