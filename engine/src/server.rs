//! The control socket: newline-delimited JSON over a Unix socket.
//!
//! Requests:  {"req": 1, "cmd": "album", "id": "AL:…"} → {"req": 1, "ok": true, "result": …}
//! Events:    {"event": "state", "state": {...}}, sent on connect and on every change.
//!
//! Pages (discover, browse, stations, playlists, collectionArtists, podcasts,
//! podcastTrending, podcast) also take `"cached": true`: their last answer,
//! saved on disk, at once and without signing in; "nothing saved" if none.
//!
//! Commands
//!   status                         current state (also pushed as events)
//!   search {query}                 artists, albums, tracks, playlists
//!   artist {id}                    bio, image, top tracks, discography, similar artists
//!   album {id}                     album with its tracks
//!   playlists                      the listener's playlists
//!   playlist {id}                  a playlist with its tracks
//!   play {source, track?, index?}  play an album, playlist, or track id; or a show's episode (source podcast:KEY)
//!   toggle | pause | resume | next | previous | replay | stop
//!   seek {position}                seconds into the track
//!   volume {level}                 0.0 to 1.0, the engine's own level
//!   stations                       the listener's radio stations
//!   station {id, name?}            play a station; id "shuffle" for Shuffle
//!   createStation {seed}           make a station from an AR:, TR:, or GE: id and play it
//!   removeStation {id}             remove a station from the account; the stations left
//!   collect {id, add}              add a song, album, or playlist to the collection, or take it out
//!   collectionArtists              the artists of the collected albums and songs
//!   uncollect {id, from}           take it out: from ["collection", "thumbs"], as listed
//!   thumbUp | thumbDown | tired    feedback on the radio track playing
//!   listenHere                     take the account over from another device
//!   signIn {user, password}        sign in, and save the account once Pandora accepts it
//!   signOut                        forget the saved account and stop
//!   browse {path?, id?}            the Browse page, a section's full list, or a mood's stations
//!   podcasts                       your shows and their newest episodes
//!   podcastTrending                shows people listen to now
//!   podcastSearch {query}          shows from Pandora and Apple's directory; a feed's address finds it
//!   podcast {key}                  a show (a PC: id or a feed's address) and its episodes
//!   subscribe {key, add}           follow a show, or stop
//!   markPlayed {id, played}        mark an episode played or not
//!   discover {refresh?}            magazine articles, matched to albums, songs, artists
//!   art {url}                      any artwork as a local file, cached
//!   openLink {url}                 where an article link leads, checked public at every hop
//!   recommend {fresh?}             songs like the one playing, from the AI agent
//!   agents                         the AI agent in use, its model and effort, and the choices
//!   setAi {model?, effort?}        Omadora's model or effort for that agent (null: the agent's own)
//!   modes {station}                a station's modes and which is on
//!   setMode {station, mode}        switch it; the next song is in the new mode

use crate::catalog;
use crate::discover;
use crate::radio;
use crate::engine::{Cmd, Shared};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

pub fn socket_path() -> Result<PathBuf> {
    Ok(crate::files::runtime_dir()?.join("engine.sock"))
}

/// The longest request line accepted; sign-in is the biggest real one.
const MAX_REQUEST: usize = 64 * 1024;

pub fn serve(shared: Arc<Shared>) -> Result<()> {
    let path = socket_path()?;
    if UnixStream::connect(&path).is_ok() {
        return Err(anyhow!("an engine is already running at {}", path.display()));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    eprintln!("omadora-engine listening on {}", path.display());
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let shared = shared.clone();
        std::thread::spawn(move || {
            let _ = connection(shared, stream);
        });
    }
    Ok(())
}

fn connection(shared: Arc<Shared>, stream: UnixStream) -> Result<()> {
    let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
    // One writer per connection; requests and events both go through it.
    let mut writer = stream.try_clone()?;
    std::thread::spawn(move || {
        for line in out_rx {
            if writer.write_all(line.as_bytes()).and_then(|_| writer.write_all(b"\n")).is_err() {
                return;
            }
        }
    });
    let events = shared.subscribe();
    let forward = out_tx.clone();
    std::thread::spawn(move || {
        for line in events {
            if forward.send(line).is_err() {
                return;
            }
        }
    });

    // One request per line, and no line longer than a request needs.
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let read = (&mut reader).take(MAX_REQUEST as u64 + 1).read_line(&mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_REQUEST {
            let _ = out_tx.send(json!({ "ok": false, "error": "request too long" }).to_string());
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = out_tx.send(json!({ "ok": false, "error": format!("bad JSON: {e}") }).to_string());
                continue;
            }
        };
        let shared = shared.clone();
        let out = out_tx.clone();
        // Catalog calls can take a while; never let one hold up a pause.
        std::thread::spawn(move || respond(&shared, &request, &out));
    }
    Ok(())
}

/// Without on demand, nothing is playable on demand, whatever the catalog
/// says of the item itself: the widget then offers its station instead.
fn radio_only(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("playable") {
                map.insert("playable".into(), Value::Bool(false));
            }
            map.values_mut().for_each(radio_only);
        }
        Value::Array(items) => items.iter_mut().for_each(radio_only),
        _ => {}
    }
}

/// Pages the widget shows at once from their last answer, saved on disk,
/// while it asks for a fresh one: so opening the player after a restart
/// doesn't wait on sign-in and the network.
const SAVED: [&str; 8] = ["discover", "browse", "stations", "playlists", "collectionArtists", "podcasts", "podcastTrending", "podcast"];

/// Where a page's last answer is kept: in the account's folder (it can
/// hold their playlists), one file per page.
fn saved_path(request: &Value) -> Option<PathBuf> {
    let cmd = request["cmd"].as_str().filter(|c| SAVED.contains(c))?;
    let which = json!([cmd, request["path"], request["id"], request["key"]]).to_string();
    let name = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(which.as_bytes()));
    Some(crate::files::account_dir()?.join("pages").join(format!("{}.json", &name[..32])))
}

fn respond(shared: &Shared, request: &Value, out: &Sender<String>) {
    let req = request["req"].clone();
    let cmd = request["cmd"].as_str().unwrap_or_default();
    // The last answer, if there is one: no sign-in, no network.
    if request["cached"] == true {
        let saved = saved_path(request).and_then(|path| std::fs::read_to_string(path).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let reply = match saved {
            Some(result) => json!({ "req": req, "ok": true, "result": result }),
            None => json!({ "req": req, "ok": false, "error": "nothing saved" }),
        };
        let _ = out.send(reply.to_string());
        return;
    }
    let catalog = matches!(cmd, "search" | "artist" | "album" | "playlist" | "playlists" | "recommend" | "discover");
    let on_demand = shared.pandora().is_none_or(|p| p.account().on_demand);
    // Where this answer is saved, decided now: an account switched while it
    // was fetched doesn't get the old account's answer.
    let (asked_by, save_to) = (crate::pandora::saved_user(), saved_path(request));
    let reply = match handle(shared, request).map(|mut result| {
        if catalog && !on_demand {
            radio_only(&mut result);
        }
        result
    }) {
        Ok(result) => {
            if let Some(path) = save_to.filter(|_| crate::pandora::saved_user() == asked_by) {
                let _ = crate::files::write_private(&path, result.to_string().as_bytes());
            }
            json!({ "req": req, "ok": true, "result": result })
        }
        Err(e) => json!({ "req": req, "ok": false, "error": format!("{e:#}") }),
    };
    let _ = out.send(reply.to_string());
}

fn arg<'a>(request: &'a Value, key: &str) -> Result<&'a str> {
    request[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("missing \"{key}\""))
}

fn handle(shared: &Shared, request: &Value) -> Result<Value> {
    let cmd = request["cmd"].as_str().unwrap_or_default();
    let control = |c: Cmd| {
        shared.send(c);
        Ok(Value::Null)
    };
    match cmd {
        "status" => {
            // The stored snapshot is stamped; bring its position up to now.
            let mut state = shared.state.lock().unwrap().clone();
            if state["status"] == "playing" {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                let elapsed = (now - state["positionAt"].as_f64().unwrap_or(now)) / 1000.0;
                state["position"] = json!(state["position"].as_f64().unwrap_or(0.0) + elapsed.max(0.0));
                state["positionAt"] = json!(now as u64);
            }
            Ok(state)
        }
        "play" => control(Cmd::Play {
            source: arg(request, "source")?.to_string(),
            track: request["track"].as_str().map(str::to_string),
            index: request["index"].as_u64().map(|i| i as usize),
        }),
        "toggle" => control(Cmd::Toggle),
        "pause" => control(Cmd::Pause),
        "resume" => control(Cmd::Resume),
        "next" => control(Cmd::Next),
        "previous" => control(Cmd::Previous),
        "replay" => control(Cmd::Replay),
        "stop" => control(Cmd::Stop),
        "seek" => control(Cmd::Seek(request["position"].as_f64().ok_or_else(|| anyhow!("missing \"position\""))?)),
        "volume" => control(Cmd::Volume(request["level"].as_f64().ok_or_else(|| anyhow!("missing \"level\""))? as f32)),
        "station" => control(Cmd::Station {
            id: arg(request, "id")?.to_string(),
            name: request["name"].as_str().unwrap_or_default().to_string(),
        }),
        "createStation" => control(Cmd::CreateStation { seed: arg(request, "seed")?.to_string() }),
        "thumbUp" => control(Cmd::ThumbUp),
        "thumbDown" => control(Cmd::ThumbDown),
        "tired" => control(Cmd::Tired),
        "listenHere" => control(Cmd::ListenHere),
        "signIn" => {
            let user = arg(request, "user")?.trim();
            let password = arg(request, "password")?;
            if user.is_empty() || password.is_empty() {
                return Err(anyhow!("Enter your email and password."));
            }
            // Checked here, on the request's own thread, so the form hears
            // whether Pandora accepted it.
            let pandora = crate::pandora::Pandora::sign_in_with(user, password)?;
            shared.send(Cmd::SignedIn(Ok(pandora)));
            Ok(json!({}))
        }
        "signOut" => {
            crate::pandora::forget();
            shared.send(Cmd::SignedOut);
            Ok(json!({}))
        }
        "recommend" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            let state = shared.state.lock().unwrap().clone();
            crate::recommend::recommend(&pandora, &state, request["fresh"].as_bool().unwrap_or(false))
        }
        "agents" => Ok(crate::recommend::agents()),
        "setAi" => crate::recommend::set_ai(request),
        "art" => Ok(json!({ "path": discover::artwork(arg(request, "url")?)?.to_string_lossy() })),
        "openLink" => Ok(json!({ "url": crate::net::checked_link(arg(request, "url")?)? })),
        "discover" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            Ok(discover::snapshot(pandora, request["refresh"].as_bool().unwrap_or(false)))
        }
        "collect" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            let id = arg(request, "id")?;
            let add = request["add"].as_bool().ok_or_else(|| anyhow!("missing \"add\""))?;
            crate::collection::set(&pandora, id, add)?;
            shared.send(Cmd::Collected { id: id.to_string(), add });
            Ok(json!({ "collected": add }))
        }
        "uncollect" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            let id = arg(request, "id")?;
            let from: Vec<String> = request["from"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect();
            crate::collection::remove(&pandora, id, &from)?;
            shared.send(Cmd::Uncollected {
                id: id.to_string(),
                collection: from.iter().any(|f| f == "collection"),
                thumbs: from.iter().any(|f| f == "thumbs"),
            });
            Ok(json!({}))
        }
        "browse" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            crate::browse::page(&pandora, request["path"].as_str(), request["id"].as_str())
        }
        "podcasts" => Ok(crate::podcasts::overview(shared.pandora())),
        "podcastTrending" => crate::podcasts::trending(),
        "podcastSearch" => crate::podcasts::search(shared.pandora().as_deref(), arg(request, "query")?),
        "podcast" => crate::podcasts::show(shared.pandora(), arg(request, "key")?),
        "subscribe" => {
            let add = request["add"].as_bool().ok_or_else(|| anyhow!("missing \"add\""))?;
            crate::podcasts::subscribe(shared.pandora(), arg(request, "key")?, add)
        }
        "markPlayed" => {
            let played = request["played"].as_bool().ok_or_else(|| anyhow!("missing \"played\""))?;
            crate::podcasts::mark(arg(request, "id")?, played);
            Ok(json!({}))
        }
        "collectionArtists" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            crate::collection::artists(&pandora)
        }
        "removeStation" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            let id = arg(request, "id")?;
            let left = radio::remove_station(&pandora, id)?;
            shared.send(Cmd::StationRemoved(id.to_string()));
            Ok(left)
        }
        "stations" | "modes" | "setMode" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            match cmd {
                "stations" => radio::stations(&pandora),
                "modes" => radio::modes(&pandora, arg(request, "station")?),
                _ => {
                    let station = arg(request, "station")?;
                    let mode = request["mode"].as_i64().ok_or_else(|| anyhow!("missing \"mode\""))?;
                    let modes = radio::set_mode(&pandora, station, mode)?;
                    if shared.state.lock().unwrap()["source"]["id"] == station {
                        shared.send(Cmd::ModeChanged);
                    }
                    Ok(modes)
                }
            }
        }
        "search" | "artist" | "album" | "playlists" | "playlist" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            match cmd {
                "search" => catalog::search(&pandora, arg(request, "query")?),
                "artist" => {
                    let id = arg(request, "id")?;
                    let mut page = catalog::artist_page(&pandora, id)?;
                    // What of theirs is in the collection, for "In your collection".
                    page["collected"] = crate::collection::for_artist(&pandora, id).unwrap_or_else(|_| json!({ "albums": [], "songs": [] }));
                    Ok(page)
                }
                "album" => catalog::album_page(&pandora, arg(request, "id")?),
                "playlist" => catalog::playlist_page(&pandora, arg(request, "id")?),
                _ => catalog::playlists(&pandora),
            }
        }
        "" => Err(anyhow!("missing \"cmd\"")),
        other => Err(anyhow!("unknown command \"{other}\"")),
    }
}
