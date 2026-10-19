//! The control socket: newline-delimited JSON over a Unix socket.
//!
//! Requests:  {"req": 1, "cmd": "album", "id": "AL:…"} → {"req": 1, "ok": true, "result": …}
//! Events:    {"event": "state", "state": {...}}, sent on connect and on every change.
//!
//! Commands
//!   status                         current state (also pushed as events)
//!   search {query}                 artists, albums, tracks, playlists
//!   artist {id}                    bio, image, top tracks, discography, similar artists
//!   album {id}                     album with its tracks
//!   playlists                      the listener's playlists
//!   playlist {id}                  a playlist with its tracks
//!   play {source, track?, index?}  play an album, playlist, or track id
//!   toggle | pause | resume | next | previous | replay | stop
//!   seek {position}                seconds into the track
//!   volume {level}                 0.0 to 1.0, the engine's own level
//!   stations                       the listener's radio stations
//!   station {id, name?}            play a station; id "shuffle" for Shuffle
//!   createStation {seed}           make a station from an AR: or TR: id and play it
//!   removeStation {id}             remove a station from the account; the stations left
//!   thumbUp | thumbDown | tired    feedback on the radio track playing
//!   listenHere                     take the account over from another device
//!   signIn {user, password}        sign in, and save the account once Pandora accepts it
//!   signOut                        forget the saved account and stop
//!   discover {refresh?}            magazine articles, matched to albums, songs, artists
//!   image {url}                    an article's picture as a local file, cached
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
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
        format!("/tmp/omadora-{uid}")
    });
    PathBuf::from(runtime).join("omadora").join("engine.sock")
}

pub fn serve(shared: Arc<Shared>) -> Result<()> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err(anyhow!("an engine is already running at {}", path.display()));
    }
    let _ = std::fs::remove_file(&path);
    std::fs::create_dir_all(path.parent().unwrap())?;
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

    for line in BufReader::new(stream).lines() {
        let line = line?;
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

fn respond(shared: &Shared, request: &Value, out: &Sender<String>) {
    let req = request["req"].clone();
    let reply = match handle(shared, request) {
        Ok(result) => json!({ "req": req, "ok": true, "result": result }),
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
        "image" => Ok(json!({ "path": discover::image(arg(request, "url")?)?.to_string_lossy() })),
        "discover" => {
            let pandora = shared.pandora().ok_or_else(|| anyhow!("still signing in"))?;
            Ok(discover::snapshot(pandora, request["refresh"].as_bool().unwrap_or(false)))
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
                "artist" => catalog::artist_page(&pandora, arg(request, "id")?),
                "album" => catalog::album_page(&pandora, arg(request, "id")?),
                "playlist" => catalog::playlist_page(&pandora, arg(request, "id")?),
                _ => catalog::playlists(&pandora),
            }
        }
        "" => Err(anyhow!("missing \"cmd\"")),
        other => Err(anyhow!("unknown command \"{other}\"")),
    }
}
