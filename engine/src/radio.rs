//! Radio: stations, their playlists, feedback, and modes, on the web API.
//!
//! Pandora Free is paid for by audio ads, which only the device ("tuner")
//! API delivers: for an ad-supported account, playlists come from there,
//! with an ad every few songs. Each such item's token says where it came
//! from ("device:STATION:TOKEN", "ad:STATION:TRACKING,…"), so feedback,
//! "tired", and play reports go back the same way; an ad is reported to
//! Pandora when it starts, as its own apps do, and can't be skipped.
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
        "canRemove": s["allowDelete"].as_bool().unwrap_or(false),
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

/// Remove a station from the listener's account, and check it's gone:
/// Pandora answers an unknown station with success, so its word alone
/// proves nothing.
pub fn remove_station(pandora: &Pandora, id: &str) -> Result<Value> {
    pandora.call("v1/station/removeStation", json!({ "stationId": id }))?;
    let left = stations(pandora)?;
    if left.as_array().into_iter().flatten().any(|s| s["id"] == id) {
        anyhow::bail!("Pandora didn't remove the station.");
    }
    Ok(left)
}

/// Shuffle plays across all stations; Pandora hands back a station for it.
pub fn shuffle(pandora: &Pandora) -> Result<Value> {
    if ad_supported(pandora) {
        // The device API's shuffle is its QuickMix station.
        let list = pandora.device_call("user.getStationList", json!({}))?;
        let quickmix = list["stations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["isQuickMix"].as_bool().unwrap_or(false))
            .ok_or_else(|| anyhow::anyhow!("Pandora has no shuffle for this account"))?;
        return Ok(json!({ "id": quickmix["stationToken"], "name": "Shuffle", "shuffle": true }));
    }
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
fn ad_supported(pandora: &Pandora) -> bool {
    pandora.account().audio_ads
}

/// A station's next songs (and, on Pandora Free, ads) from the device API.
fn device_fragment(pandora: &Pandora, station_id: &str) -> Result<Vec<Value>> {
    let found = pandora.device_call("station.getPlaylist", json!({ "stationToken": station_id, "includeTrackLength": true }))?;
    let items: Vec<Value> = found["items"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = items.iter().filter_map(|t| t["pandoraId"].as_str().map(str::to_string)).collect();
    let annotations = catalog::annotate(pandora, &ids).unwrap_or_default();
    let mut out = Vec::new();
    for item in &items {
        if let Some(ad) = item["adToken"].as_str() {
            // An ad that can't be fetched is dropped; the songs still play.
            if let Ok(ad) = device_ad(pandora, station_id, ad) {
                out.push(ad);
            }
            continue;
        }
        let audio = &item["audioUrlMap"]["highQuality"];
        let url = audio["audioUrl"].as_str().unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        let mut track = match annotations.get(item["pandoraId"].as_str().unwrap_or_default()) {
            Some(a) => catalog::track(a),
            None => json!({ "id": item["pandoraId"], "artist": item["artistName"], "album": item["albumName"] }),
        };
        track["title"] = item["songName"].clone();
        track["duration"] = item["trackLength"].clone();
        if track["art"].as_str().unwrap_or_default().is_empty() {
            track["art"] = item["albumArtUrl"].clone();
        }
        let rating = item["songRating"].as_i64().unwrap_or(0);
        track["kind"] = json!("song");
        track["rating"] = json!(if rating > 0 { 1 } else { 0 });
        track["canRate"] = json!(item["allowFeedback"].as_bool().unwrap_or(false));
        track["canTire"] = json!(true);
        track["canSkip"] = json!(true);
        track["canReplay"] = json!(false);
        track["token"] = json!(format!("device:{station_id}:{}", item["trackToken"].as_str().unwrap_or_default()));
        track["audioUrl"] = json!(url);
        track["encoding"] = audio["encoding"].clone();
        track["gain"] = item["trackGain"].clone();
        out.push(track);
    }
    Ok(out)
}

/// An ad, ready to play: its audio and the tokens that report it.
fn device_ad(pandora: &Pandora, station_id: &str, ad_token: &str) -> Result<Value> {
    let meta = pandora.device_call(
        "ad.getAdMetadata",
        json!({ "adToken": ad_token, "returnAdTrackingTokens": true, "supportAudioAds": true, "includeBannerAd": false }),
    )?;
    let audio = &meta["audioUrlMap"]["highQuality"];
    let url = audio["audioUrl"].as_str().filter(|u| !u.is_empty()).ok_or_else(|| anyhow::anyhow!("the ad has no audio"))?;
    let tracking: Vec<&str> = meta["adTrackingTokens"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).collect();
    let title = meta["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("Advertisement");
    Ok(json!({
        "kind": "ad",
        "id": "",
        "title": title,
        "artist": meta["companyName"].as_str().unwrap_or_default(),
        "album": "",
        "art": meta["imageUrl"].as_str().unwrap_or_default(),
        "rating": 0,
        "canRate": false,
        "canTire": false,
        "canSkip": false,
        "canReplay": false,
        "token": format!("ad:{station_id}:{}", tracking.join(",")),
        "audioUrl": url,
        "encoding": audio["encoding"].as_str().unwrap_or("aacplus"),
        "gain": "0",
    }))
}

/// "device:STATION:TOKEN" or "ad:STATION:TRACKING" → (kind, station, rest).
fn device_token(token: &str) -> Option<(&str, &str, &str)> {
    let (kind, rest) = token.split_once(':')?;
    if kind != "device" && kind != "ad" {
        return None;
    }
    let (station, value) = rest.split_once(':')?;
    Some((kind, station, value))
}

pub fn fragment(pandora: &Pandora, station_id: &str, starting: bool) -> Result<Vec<Value>> {
    if ad_supported(pandora) {
        return device_fragment(pandora, station_id);
    }
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
    if let Some(("device", station, track)) = device_token(token) {
        pandora.device_call("station.addFeedback", json!({ "stationToken": station, "trackToken": track, "isPositive": positive }))?;
        return Ok(());
    }
    pandora.call("v1/station/addFeedback", json!({ "trackToken": token, "isPositive": positive }))?;
    Ok(())
}

pub fn tired(pandora: &Pandora, token: &str) -> Result<()> {
    if let Some(("device", _, track)) = device_token(token) {
        pandora.device_call("user.sleepSong", json!({ "trackToken": track }))?;
        return Ok(());
    }
    pandora.call("v1/listener/addTiredSong", json!({ "trackToken": token }))?;
    Ok(())
}

/// A song or ad has started. An ad is reported to Pandora here, which is
/// what it counts; device songs need no report.
pub fn track_started(pandora: &Pandora, token: &str) {
    match device_token(token) {
        Some(("ad", station, tracking)) => {
            let tokens: Vec<&str> = tracking.split(',').filter(|t| !t.is_empty()).collect();
            let _ = pandora.device_call("ad.registerAd", json!({ "stationId": station, "adTrackingTokens": tokens }));
        }
        Some(_) => {}
        None => {
            let _ = pandora.call("v1/station/trackStarted", json!({ "trackToken": token }));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_tokens_say_where_they_came_from() {
        assert_eq!(device_token("device:172:abc"), Some(("device", "172", "abc")));
        assert_eq!(device_token("ad:172:t1,t2"), Some(("ad", "172", "t1,t2")));
        assert_eq!(device_token("plainwebtoken"), None);
        assert_eq!(device_token("other:1:2"), None);
    }
}
