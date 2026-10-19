//! The player: what is playing, what comes next, and the audio.
//!
//! Two kinds of listening share one queue. On demand, the queue is an album,
//! playlist, or song, fixed up front. On radio, it is a station's tracks,
//! fetched a few at a time and topped up as they play.
//!
//! Where the listener was (source, song, position) is saved as it changes
//! and restored, paused, when the engine starts again. A restore makes no
//! playback requests until play is pressed, so it never takes the account's
//! one stream from another device.
//!
//! One thread owns all of it and takes `Cmd`s off a channel. Network work
//! (sign-in, track lists, audio downloads) runs on helper threads that send
//! their results back as commands, tagged with a generation so results for
//! something the listener has already moved past are dropped.

use crate::audio::{Audio, decode_with_ffmpeg};
use crate::catalog;
use crate::pandora::Pandora;
use crate::radio::{self, Busy};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A downloaded track, ready to decode.
#[derive(Clone)]
pub struct Loaded {
    bytes: Arc<Vec<u8>>,
    encoding: String,
    gain: f32,
    receipt: String,
}

pub enum Cmd {
    // From clients.
    Play { source: String, track: Option<String>, index: Option<usize> },
    Station { id: String, name: String },
    CreateStation { seed: String },
    Toggle,
    Pause,
    Resume,
    Next,
    Previous,
    Seek(f64),
    Replay,
    Stop,
    Volume(f32),
    ThumbUp,
    ThumbDown,
    Tired,
    ModeChanged,
    ListenHere,
    SignedOut,
    // From helper threads.
    SignedIn(Result<Arc<Pandora>, String>),
    Queued { generation: u64, source: String, name: String, tracks: Vec<Value>, track: Option<String>, index: Option<usize> },
    StationReady { generation: u64, station: Value, tracks: Vec<Value> },
    More { generation: u64, result: Result<Vec<Value>, (String, bool)> },
    QueueFailed { generation: u64, error: String },
    Loaded { generation: u64, index: usize, result: Result<Loaded, String> },
    Busy { generation: u64, message: String },
    Prefetched { key: String, loaded: Loaded },
    Restored { generation: u64, source: String, name: String, tracks: Vec<Value>, track: String, position: f64 },
    Rated { token: String, rating: i64 },
    Failed(String),
}

/// What clients share with the engine thread.
pub struct Shared {
    pub pandora: RwLock<Option<Arc<Pandora>>>,
    pub state: Mutex<Value>,
    pub subscribers: Mutex<Vec<Sender<String>>>,
    pub commands: Mutex<Sender<Cmd>>,
}

impl Shared {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.commands.lock().unwrap().send(cmd);
    }

    pub fn pandora(&self) -> Option<Arc<Pandora>> {
        self.pandora.read().unwrap().clone()
    }

    pub fn subscribe(&self) -> Receiver<String> {
        let (tx, rx) = channel();
        let snapshot = self.state.lock().unwrap().clone();
        let _ = tx.send(json!({ "event": "state", "state": snapshot }).to_string());
        self.subscribers.lock().unwrap().push(tx);
        rx
    }
}

/// Where we are in the track. Kept here rather than read from the audio
/// player, whose position lags a moment behind loads and seeks.
#[derive(Default)]
struct Clock {
    base: f64,
    since: Option<Instant>,
}

impl Clock {
    fn at(&self) -> f64 {
        self.base + self.since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0)
    }

    fn set(&mut self, position: f64, running: bool) {
        self.base = position;
        self.since = running.then(Instant::now);
    }

    fn stop(&mut self) {
        self.base = self.at();
        self.since = None;
    }

    fn run(&mut self) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Loading => "loading",
            Status::Playing => "playing",
            Status::Paused => "paused",
        }
    }
}

/// Fields the engine needs from a radio track but clients never see.
const PRIVATE: [&str; 6] = ["token", "audioUrl", "encoding", "gain", "receipt", "skipUrl"];

fn public(track: &Value) -> Value {
    let mut out = track.clone();
    if let Some(map) = out.as_object_mut() {
        for key in PRIVATE {
            map.remove(key);
        }
    }
    out
}

/// Radio can play the same song twice, so its cache key is the play token.
fn cache_key(track: &Value) -> String {
    track["token"].as_str().or(track["id"].as_str()).unwrap_or_default().to_string()
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Queue the next track in the audio this many seconds before the end.
const QUEUE_AHEAD: f64 = 8.0;

// Fetch more radio when this many tracks or fewer are left.
const RADIO_LOW_WATER: usize = 2;
// Played radio tracks are dropped from the queue past this many.
const RADIO_KEEP_BEHIND: usize = 1;
const HISTORY_LENGTH: usize = 20;

struct Engine {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    audio: Audio,
    status: Status,
    signing_in: bool,
    retry_sign_in: Option<Instant>,
    // No account is saved; the widget asks for one.
    needs_account: bool,
    radio: bool,
    station: Value,
    source: String,
    source_name: String,
    queue: Vec<Value>,
    index: Option<usize>,
    history: Vec<Value>,
    generation: u64,
    fetching: bool,
    waiting_for_more: bool,
    cache: HashMap<String, Loaded>,
    error: String,
    // Another device holds the account; "listen here" takes it over.
    busy: bool,
    // The current song's audio is loaded. A restored session shows the song
    // paused before any audio is fetched.
    audio_ready: bool,
    // Where to start the next song that loads, for resuming a restore.
    resume_at: Option<f64>,
    restored: bool,
    // A restored station has not started this session; its first fetch
    // must say so.
    cold: bool,
    // The next track, already queued in the audio to follow without a gap.
    queued_next: Option<usize>,
    saved: String,
    last_save: Instant,
    clock: Clock,
    last_publish: Instant,
}

pub fn start() -> anyhow::Result<Arc<Shared>> {
    let (tx, rx) = channel();
    let shared = Arc::new(Shared {
        pandora: RwLock::new(None),
        state: Mutex::new(Value::Null),
        subscribers: Mutex::new(Vec::new()),
        commands: Mutex::new(tx.clone()),
    });
    let audio = Audio::open()?;
    let mut engine = Engine {
        shared: shared.clone(),
        tx,
        audio,
        status: Status::Idle,
        signing_in: false,
        retry_sign_in: None,
        needs_account: false,
        radio: false,
        station: Value::Null,
        source: String::new(),
        source_name: String::new(),
        queue: Vec::new(),
        index: None,
        history: Vec::new(),
        generation: 0,
        fetching: false,
        waiting_for_more: false,
        cache: HashMap::new(),
        error: String::new(),
        busy: false,
        audio_ready: false,
        resume_at: None,
        restored: false,
        cold: false,
        queued_next: None,
        saved: String::new(),
        last_save: Instant::now(),
        clock: Clock::default(),
        last_publish: Instant::now(),
    };
    engine.sign_in();
    engine.publish();
    std::thread::Builder::new().name("engine".into()).spawn(move || engine.run(rx))?;
    Ok(shared)
}

impl Engine {
    fn run(mut self, rx: Receiver<Cmd>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        if self.status == Status::Playing {
            match self.queued_next {
                // The queued track has taken over: carry on without reloading.
                Some(next) if self.audio.loaded() <= 1 => {
                    self.queued_next = None;
                    self.carry_on(next);
                }
                Some(_) => {}
                None => self.queue_next(),
            }
        }
        if self.status == Status::Playing && self.audio.finished() {
            self.advance(1, false);
        }
        if self.status == Status::Playing {
            self.save();
        }
        if self.retry_sign_in.is_some_and(|at| Instant::now() >= at) {
            self.sign_in();
        }
        // Clients extrapolate the position; a periodic snapshot corrects drift.
        if self.status == Status::Playing && self.last_publish.elapsed() > Duration::from_secs(10) {
            self.publish();
        }
    }

    fn sign_in(&mut self) {
        if self.signing_in {
            return;
        }
        self.signing_in = true;
        self.retry_sign_in = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = Pandora::sign_in().map_err(|e| format!("{e:#}"));
            let _ = tx.send(Cmd::SignedIn(result));
        });
    }

    fn current(&self) -> Option<&Value> {
        self.index.and_then(|i| self.queue.get(i))
    }

    fn current_allows(&self, right: &str) -> bool {
        // On demand, everything is allowed; radio tracks carry their rights.
        !self.radio || self.current().is_some_and(|t| t[right].as_bool().unwrap_or(false))
    }

    fn fail(&mut self, error: String) {
        self.error = error;
        self.publish();
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::SignedIn(result) => {
                self.signing_in = false;
                match result {
                    Ok(pandora) => {
                        *self.shared.pandora.write().unwrap() = Some(pandora);
                        self.needs_account = false;
                        self.retry_sign_in = None;
                        self.error.clear();
                        if self.source.is_empty() && self.queue.is_empty() {
                            self.restore();
                        }
                    }
                    // Nothing to sign in with: wait for the widget's form.
                    Err(error) if error.contains(crate::pandora::NO_ACCOUNT) => {
                        self.needs_account = true;
                        self.error.clear();
                    }
                    Err(error) => {
                        self.error = error;
                        self.retry_sign_in = Some(Instant::now() + Duration::from_secs(30));
                    }
                }
                self.publish();
            }
            Cmd::SignedOut => {
                // The account is forgotten already; drop everything it played.
                self.generation += 1;
                self.stop_audio();
                self.clock.set(0.0, false);
                self.status = Status::Idle;
                *self.shared.pandora.write().unwrap() = None;
                self.radio = false;
                self.station = Value::Null;
                self.source.clear();
                self.source_name.clear();
                self.queue.clear();
                self.index = None;
                self.history.clear();
                self.cache.clear();
                self.error.clear();
                self.busy = false;
                self.retry_sign_in = None;
                self.needs_account = true;
                let _ = std::fs::remove_file(resume_path());
                self.publish();
            }
            Cmd::Play { source, track, index } => self.play(source, track, index),
            Cmd::Station { id, name } => self.start_station(id, name, None),
            Cmd::CreateStation { seed } => self.start_station(String::new(), String::new(), Some(seed)),
            Cmd::Queued { generation, source, name, tracks, track, index } => {
                if generation != self.generation {
                    return;
                }
                let at = track
                    .and_then(|id| tracks.iter().position(|t| t["id"] == id.as_str()))
                    .or(index)
                    .unwrap_or(0);
                self.radio = false;
                self.station = Value::Null;
                self.source = source;
                self.source_name = name;
                self.queue = tracks;
                self.cache.clear();
                if self.queue.is_empty() {
                    self.status = Status::Idle;
                    self.fail("Nothing to play there.".into());
                } else {
                    self.load(at.min(self.queue.len() - 1));
                }
            }
            Cmd::StationReady { generation, station, tracks } => {
                if generation != self.generation {
                    return;
                }
                self.radio = true;
                self.cold = false;
                self.restored = false;
                self.source = station["id"].as_str().unwrap_or_default().to_string();
                self.source_name = station["name"].as_str().unwrap_or_default().to_string();
                self.station = station;
                self.queue = tracks;
                self.cache.clear();
                self.fetching = false;
                self.waiting_for_more = false;
                if self.queue.is_empty() {
                    self.status = Status::Idle;
                    self.fail("Nothing came back to play on this station.".into());
                } else {
                    self.load(0);
                }
            }
            Cmd::More { generation, result } => {
                if generation != self.generation || !self.radio {
                    return;
                }
                self.fetching = false;
                match result {
                    Ok(tracks) => {
                        self.cold = false;
                        self.queue.extend(tracks);
                        if self.waiting_for_more {
                            self.waiting_for_more = false;
                            self.advance(1, false);
                        } else {
                            if let Some(i) = self.index {
                                self.prefetch(i + 1);
                            }
                            self.publish();
                        }
                    }
                    Err((error, busy)) => {
                        if busy {
                            // Someone else took the account: stop, like the apps do.
                            self.generation += 1;
                            self.stop_audio();
                            self.clock.set(0.0, false);
                            self.status = Status::Idle;
                            self.busy = true;
                        }
                        self.waiting_for_more = false;
                        self.fail(error);
                    }
                }
            }
            Cmd::Busy { generation, message } => {
                if generation == self.generation {
                    self.stop_audio();
                    self.clock.set(0.0, false);
                    self.status = Status::Idle;
                    self.busy = true;
                    self.fail(message);
                }
            }
            Cmd::ListenHere => {
                if !self.radio || self.source.is_empty() {
                    return;
                }
                self.busy = false;
                let (id, name) = (self.source.clone(), self.source_name.clone());
                let tx = self.tx.clone();
                self.background(move |pandora| {
                    radio::playback_resumed(pandora, true)?;
                    let _ = tx.send(Cmd::Station { id, name });
                    Ok(None)
                });
            }
            Cmd::QueueFailed { generation, error } => {
                if generation == self.generation {
                    self.status = Status::Idle;
                    self.fail(error);
                }
            }
            Cmd::Loaded { generation, index, result } => {
                if generation != self.generation {
                    return;
                }
                match result {
                    Ok(loaded) => self.begin(index, loaded),
                    Err(error) => {
                        if self.restored && self.radio {
                            // The saved song's audio link has expired; start
                            // the station fresh rather than show an error.
                            self.restored = false;
                            self.resume_at = None;
                            let (id, name) = (self.source.clone(), self.source_name.clone());
                            self.start_station(id, name, None);
                            return;
                        }
                        self.status = Status::Idle;
                        self.fail(error);
                    }
                }
            }
            Cmd::Restored { generation, source, name, tracks, track, position } => {
                // Dropped if the listener started something meanwhile.
                if generation != self.generation || !self.source.is_empty() || tracks.is_empty() {
                    return;
                }
                let found = tracks.iter().position(|t| t["id"] == track.as_str());
                self.radio = false;
                self.station = Value::Null;
                self.source = source;
                self.source_name = name;
                self.queue = tracks;
                self.index = Some(found.unwrap_or(0));
                self.show_restored(if found.is_some() { position } else { 0.0 });
            }
            Cmd::Prefetched { key, loaded } => {
                self.cache.insert(key, loaded);
            }
            Cmd::Rated { token, rating } => {
                for t in self.queue.iter_mut().chain(self.history.iter_mut()) {
                    if t["token"] == token.as_str() {
                        t["rating"] = json!(rating);
                    }
                }
                self.publish();
            }
            Cmd::Failed(error) => self.fail(error),
            Cmd::Toggle => match self.status {
                Status::Playing => self.pause(),
                Status::Paused => self.resume(),
                Status::Idle => {
                    if self.radio && !self.source.is_empty() {
                        let (id, name) = (self.source.clone(), self.source_name.clone());
                        self.start_station(id, name, None);
                    } else if let Some(i) = self.index {
                        self.load(i);
                    }
                }
                Status::Loading => {}
            },
            Cmd::Pause => self.pause(),
            Cmd::Resume => self.resume(),
            Cmd::Next => {
                if self.current().is_none() {
                    return;
                }
                if self.radio && !self.current_allows("canSkip") {
                    self.fail("You've used this station's skips for now.".into());
                    return;
                }
                if self.radio
                    && let Some(url) = self.current().and_then(|t| t["skipUrl"].as_str()).filter(|u| !u.is_empty())
                {
                    self.ping(url.to_string());
                }
                self.advance(1, true);
            }
            Cmd::Previous => {
                if self.radio || self.clock.at() > 3.0 || self.index == Some(0) {
                    // Radio has no going back; only a replay of this song.
                    if self.current_allows("canReplay") {
                        self.seek(0.0);
                    }
                } else {
                    self.advance(-1, true);
                }
            }
            Cmd::Replay => {
                if self.current_allows("canReplay") {
                    self.seek(0.0);
                    self.resume();
                }
            }
            Cmd::Seek(seconds) => {
                if self.current_allows("canReplay") {
                    self.seek(seconds);
                }
            }
            Cmd::Stop => {
                self.generation += 1;
                self.stop_audio();
                self.clock.set(0.0, false);
                self.status = Status::Idle;
                self.publish();
            }
            Cmd::Volume(v) => {
                self.audio.set_volume(v);
                self.publish();
            }
            Cmd::ThumbUp => self.rate(true),
            Cmd::ThumbDown => {
                if self.current_token("canRate").is_some() {
                    self.rate(false);
                    // A thumbs down also moves on, as in Pandora's apps.
                    self.advance(1, true);
                }
            }
            Cmd::Tired => {
                let Some(token) = self.current_token("canTire") else { return };
                self.background(move |pandora| radio::tired(pandora, &token).map(|_| None));
                self.advance(1, true);
            }
            Cmd::ModeChanged => {
                // Tracks already fetched are in the old mode; drop them.
                if self.radio
                    && let Some(i) = self.index
                {
                    self.queue.truncate(i + 1);
                    self.cache.retain(|_, _| false);
                    self.fetch_more();
                    self.publish();
                }
            }
        }
    }

    /// Run a Pandora call off the engine thread. Its result may update state.
    fn background<F>(&self, work: F)
    where
        F: FnOnce(&Pandora) -> anyhow::Result<Option<Cmd>> + Send + 'static,
    {
        let Some(pandora) = self.shared.pandora() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || match work(&pandora) {
            Ok(Some(cmd)) => {
                let _ = tx.send(cmd);
            }
            Ok(None) => {}
            Err(e) => {
                let _ = tx.send(Cmd::Failed(format!("{e:#}")));
            }
        });
    }

    fn ping(&self, url: String) {
        if let Some(pandora) = self.shared.pandora() {
            std::thread::spawn(move || pandora.ping(&url));
        }
    }

    fn current_token(&self, right: &str) -> Option<String> {
        if !self.radio || !self.current_allows(right) {
            return None;
        }
        self.current().and_then(|t| t["token"].as_str()).map(str::to_string)
    }

    fn rate(&mut self, positive: bool) {
        let Some(token) = self.current_token("canRate") else { return };
        let rating = if positive { 1 } else { -1 };
        self.background(move |pandora| {
            radio::feedback(pandora, &token, positive)?;
            Ok(Some(Cmd::Rated { token, rating }))
        });
    }

    fn pandora(&mut self) -> Option<Arc<Pandora>> {
        let found = self.shared.pandora();
        if found.is_none() {
            if self.needs_account {
                self.fail("Sign in first.".into());
            } else {
                self.sign_in();
                self.fail("Still signing in.".into());
            }
        }
        found
    }

    fn play(&mut self, source: String, track: Option<String>, index: Option<usize>) {
        // Same album or playlist already queued: just jump within it.
        if !self.radio && source == self.source && !self.queue.is_empty() {
            let at = track
                .as_deref()
                .and_then(|id| self.queue.iter().position(|t| t["id"] == id))
                .or(index)
                .unwrap_or(0);
            self.load(at.min(self.queue.len() - 1));
            return;
        }
        let Some(pandora) = self.pandora() else { return };
        self.generation += 1;
        let generation = self.generation;
        self.begin_loading();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let cmd = match catalog::tracks_for(&pandora, &source) {
                Ok((name, tracks)) => Cmd::Queued { generation, source, name, tracks, track, index },
                Err(e) => Cmd::QueueFailed { generation, error: format!("{e:#}") },
            };
            let _ = tx.send(cmd);
        });
    }

    /// Start a station by id ("shuffle" for Shuffle), or create one from a
    /// seed (an artist or song id) and start that.
    fn start_station(&mut self, id: String, name: String, seed: Option<String>) {
        let Some(pandora) = self.pandora() else { return };
        if seed.is_none() {
            self.radio = true;
            self.source = id.clone();
            self.source_name = name.clone();
        }
        self.busy = false;
        self.generation += 1;
        let generation = self.generation;
        self.begin_loading();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<(Value, Vec<Value>)> {
                let station = match (&seed, id.as_str()) {
                    (Some(seed), _) => radio::create(&pandora, seed)?,
                    (None, "shuffle") => radio::shuffle(&pandora)?,
                    (None, _) => json!({ "id": id, "name": name }),
                };
                let sid = station["id"].as_str().unwrap_or_default().to_string();
                let tracks = radio::fragment(&pandora, &sid, true)?;
                Ok((station, tracks))
            })();
            let cmd = match result {
                Ok((station, tracks)) => Cmd::StationReady { generation, station, tracks },
                Err(e) if e.downcast_ref::<Busy>().is_some() => Cmd::Busy { generation, message: format!("{e:#}") },
                Err(e) => Cmd::QueueFailed { generation, error: format!("{e:#}") },
            };
            let _ = tx.send(cmd);
        });
    }

    fn begin_loading(&mut self) {
        self.stop_audio();
        self.clock.set(0.0, false);
        self.status = Status::Loading;
        self.error.clear();
        self.publish();
    }

    fn fetch_more(&mut self) {
        if self.fetching || !self.radio {
            return;
        }
        let Some(pandora) = self.shared.pandora() else { return };
        self.fetching = true;
        let (tx, generation, sid, starting) = (self.tx.clone(), self.generation, self.source.clone(), self.cold);
        std::thread::spawn(move || {
            let result = radio::fragment(&pandora, &sid, starting).map_err(|e| {
                let busy = e.downcast_ref::<Busy>().is_some();
                (format!("{e:#}"), busy)
            });
            let _ = tx.send(Cmd::More { generation, result });
        });
    }

    /// Fetch and start the track at `index`.
    fn load(&mut self, index: usize) {
        let Some(track) = self.queue.get(index).cloned() else { return };
        self.generation += 1;
        self.index = Some(index);
        self.error.clear();
        if let Some(loaded) = self.cache.get(&cache_key(&track)).cloned() {
            self.begin(index, loaded);
            return;
        }
        let Some(pandora) = self.pandora() else { return };
        self.stop_audio();
        self.clock.set(0.0, false);
        self.status = Status::Loading;
        self.publish();
        let (tx, generation, source) = (self.tx.clone(), self.generation, self.source.clone());
        std::thread::spawn(move || {
            let result = fetch(&pandora, &track, &source).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Cmd::Loaded { generation, index, result });
        });
    }

    fn begin(&mut self, index: usize, loaded: Loaded) {
        if let Err(e) = self.audio.load(loaded.bytes.clone(), &loaded.encoding, loaded.gain) {
            self.status = Status::Idle;
            self.fail(format!("{e:#}"));
            return;
        }
        self.queued_next = None;
        // Resuming a restored session picks up where it left off.
        let start = self.resume_at.take().unwrap_or(0.0);
        if start > 0.0 {
            let _ = self.audio.seek(Duration::from_secs_f64(start));
        }
        self.audio.play();
        self.started(index, loaded, start);
    }

    /// Near the end of a track, queue the next one in the audio so it
    /// starts the instant this one ends: no polling gap, no reload.
    fn queue_next(&mut self) {
        let Some(index) = self.index else { return };
        let Some(next) = self.queue.get(index + 1) else { return };
        let Some(loaded) = self.cache.get(&cache_key(next)).cloned() else { return };
        let length = self.current().and_then(|t| t["duration"].as_f64()).unwrap_or(0.0);
        if length <= 0.0 || length - self.clock.at() > QUEUE_AHEAD {
            return;
        }
        if self.audio.queue_next(loaded.bytes, &loaded.encoding, loaded.gain).is_ok() {
            self.queued_next = Some(index + 1);
        }
    }

    /// The queued track is playing now; catch the engine up.
    fn carry_on(&mut self, index: usize) {
        let Some(track) = self.queue.get(index).cloned() else { return };
        let Some(loaded) = self.cache.get(&cache_key(&track)).cloned() else { return };
        self.generation += 1;
        self.error.clear();
        // Noticed on the next tick, a moment in.
        let at = self.audio.position().as_secs_f64().min(1.0);
        self.started(index, loaded, at);
    }

    /// A track has started: the clock, the play report, what to fetch next.
    fn started(&mut self, index: usize, loaded: Loaded, start: f64) {
        self.audio_ready = true;
        self.restored = false;
        self.clock.set(start, true);
        self.status = Status::Playing;
        self.index = Some(index);
        // Report the play the way Pandora's own apps do.
        if !loaded.receipt.is_empty() {
            self.ping(loaded.receipt.clone());
        }
        if self.radio {
            if let Some(token) = self.queue[index]["token"].as_str().map(str::to_string) {
                self.background(move |pandora| {
                    radio::track_started(pandora, &token);
                    Ok(None)
                });
            }
            self.remember(index);
        }
        let key = cache_key(&self.queue[index]);
        let index = self.trim(index);
        self.prefetch(index + 1);
        if self.radio && self.queue.len() - index - 1 < RADIO_LOW_WATER {
            self.fetch_more();
        }
        // Keep only what is playing and what is next.
        let keep: Vec<String> = [index, index + 1].iter().filter_map(|i| self.queue.get(*i)).map(cache_key).collect();
        self.cache.retain(|k, _| keep.contains(k));
        self.cache.insert(key, loaded);
        self.publish();
    }

    /// Radio's song history, newest first.
    fn remember(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        let Some(previous) = self.queue.get(index - 1).cloned() else { return };
        if previous["kind"] != "song" || self.history.first().is_some_and(|h| h["token"] == previous["token"]) {
            return;
        }
        self.history.insert(0, previous);
        self.history.truncate(HISTORY_LENGTH);
    }

    /// Drop radio tracks long played, so the queue does not grow forever.
    /// Returns where the current track now sits.
    fn trim(&mut self, index: usize) -> usize {
        if !self.radio || index <= RADIO_KEEP_BEHIND {
            return index;
        }
        let drop = index - RADIO_KEEP_BEHIND;
        self.queue.drain(..drop);
        self.index = Some(index - drop);
        index - drop
    }

    fn prefetch(&mut self, index: usize) {
        let Some(track) = self.queue.get(index).cloned() else { return };
        let key = cache_key(&track);
        if self.cache.contains_key(&key) {
            return;
        }
        let Some(pandora) = self.shared.pandora() else { return };
        let (tx, source) = (self.tx.clone(), self.source.clone());
        std::thread::spawn(move || {
            if let Ok(loaded) = fetch(&pandora, &track, &source) {
                let _ = tx.send(Cmd::Prefetched { key, loaded });
            }
        });
    }

    fn advance(&mut self, step: isize, by_listener: bool) {
        let Some(current) = self.index else { return };
        let next = current as isize + step;
        if next >= 0 && (next as usize) < self.queue.len() {
            self.load(next as usize);
            return;
        }
        if self.radio && step > 0 {
            // Out of fetched tracks: wait for the next few.
            self.waiting_for_more = true;
            self.fetch_more();
            self.stop_audio();
            self.clock.set(0.0, false);
            self.status = Status::Loading;
            self.publish();
            return;
        }
        // End of the album or playlist.
        if !by_listener || step > 0 {
            self.stop_audio();
            self.clock.set(0.0, false);
            self.status = Status::Idle;
            self.index = Some(0);
            self.publish();
        }
    }

    fn pause(&mut self) {
        if self.status == Status::Playing {
            self.audio.pause();
            self.clock.stop();
            self.status = Status::Paused;
            if self.radio {
                self.background(|pandora| {
                    radio::playback_paused(pandora);
                    Ok(None)
                });
            }
            self.publish();
        }
    }

    fn resume(&mut self) {
        // A restored session has no audio yet: fetch it now, from the spot
        // it was left at.
        if self.status == Status::Paused && !self.audio_ready {
            if let Some(i) = self.index {
                self.resume_at = Some(self.clock.at());
                self.load(i);
            }
            return;
        }
        if self.status == Status::Paused {
            self.audio.play();
            self.clock.run();
            self.status = Status::Playing;
            if self.radio {
                // If another device has the account now, this pauses again.
                let tx = self.tx.clone();
                let generation = self.generation;
                self.background(move |pandora| {
                    if let Err(e) = radio::playback_resumed(pandora, false)
                        && e.downcast_ref::<Busy>().is_some()
                    {
                        let _ = tx.send(Cmd::Busy { generation, message: format!("{e:#}") });
                    }
                    Ok(None)
                });
            }
            self.publish();
        }
    }

    fn seek(&mut self, seconds: f64) {
        if matches!(self.status, Status::Playing | Status::Paused) {
            let length = self.current().and_then(|t| t["duration"].as_f64());
            let target = seconds.clamp(0.0, length.unwrap_or(f64::MAX));
            if !self.audio_ready {
                // Restored and not yet loaded: just move where it will start.
                self.clock.set(target, false);
                self.publish();
                return;
            }
            match self.audio.seek(Duration::from_secs_f64(target)) {
                Ok(()) => self.clock.set(target, self.status == Status::Playing),
                Err(e) => self.error = format!("{e:#}"),
            }
            self.publish();
        }
    }

    fn stop_audio(&mut self) {
        self.audio.stop();
        self.audio_ready = false;
        self.queued_next = None;
    }

    /// Put back what was playing when the engine last ran, paused. Radio
    /// comes back at once; an album or playlist re-reads its track list
    /// first (a catalog lookup, not playback).
    fn restore(&mut self) {
        let Some(saved) = read_resume() else { return };
        let track = saved["track"].clone();
        let source = saved["source"]["id"].as_str().unwrap_or_default().to_string();
        let name = saved["source"]["name"].as_str().unwrap_or_default().to_string();
        if track.is_null() || source.is_empty() {
            return;
        }
        let position = saved["position"].as_f64().unwrap_or(0.0);
        self.history = saved["history"].as_array().cloned().unwrap_or_default();
        if saved["mode"] == "radio" {
            self.radio = true;
            self.cold = true;
            self.station = saved["station"].clone();
            self.source = source;
            self.source_name = name;
            self.queue = vec![track];
            self.index = Some(0);
            self.show_restored(position);
            return;
        }
        let Some(pandora) = self.shared.pandora() else { return };
        self.generation += 1;
        let (tx, generation) = (self.tx.clone(), self.generation);
        let track = track["id"].as_str().unwrap_or_default().to_string();
        std::thread::spawn(move || {
            if let Ok((name, tracks)) = catalog::tracks_for(&pandora, &source) {
                let _ = tx.send(Cmd::Restored { generation, source, name, tracks, track, position });
            }
        });
    }

    fn show_restored(&mut self, position: f64) {
        let length = self.current().and_then(|t| t["duration"].as_f64()).unwrap_or(f64::MAX);
        self.audio_ready = false;
        self.restored = true;
        self.status = Status::Paused;
        self.clock.set(position.clamp(0.0, length), false);
        self.publish();
    }

    /// Remember where the listener is: on a new song, source, or status, and
    /// every few seconds while playing.
    fn save(&mut self) {
        let Some(track) = self.current() else { return };
        let signature = format!("{}|{}|{}", self.source, cache_key(track), self.status.name());
        if signature == self.saved && self.last_save.elapsed() < Duration::from_secs(5) {
            return;
        }
        let state = json!({
            "mode": if self.radio { "radio" } else { "ondemand" },
            "source": { "id": self.source, "name": self.source_name },
            "station": self.station,
            // Radio keeps its private fields, so a quick restart can resume
            // the very same play.
            "track": track,
            "position": self.clock.at(),
            "history": self.history,
            "savedAt": now_ms() as u64,
        });
        write_resume(&state);
        self.saved = signature;
        self.last_save = Instant::now();
    }

    fn publish(&mut self) {
        let account = self.shared.pandora().map(|p| p.account());
        let track = self.current().map(public).unwrap_or(Value::Null);
        let state = json!({
            "status": self.status.name(),
            "mode": if self.source.is_empty() { "" } else if self.radio { "radio" } else { "ondemand" },
            "signedIn": account.is_some(),
            "needsAccount": self.needs_account,
            "user": if account.is_some() { crate::pandora::saved_user() } else { None },
            "account": account.map(|a| json!({ "subscriber": a.subscriber, "audioAds": a.audio_ads })),
            "source": { "id": self.source, "name": self.source_name },
            "station": if self.radio { self.station.clone() } else { Value::Null },
            "track": track,
            "index": self.index,
            "queue": self.queue.iter().map(public).collect::<Vec<_>>(),
            "history": self.history.iter().map(public).collect::<Vec<_>>(),
            "position": self.clock.at(),
            "positionAt": now_ms() as u64,
            "volume": self.audio.volume(),
            "error": self.error,
            "busy": self.busy,
        });
        self.save();
        let line = json!({ "event": "state", "state": state }).to_string();
        *self.shared.state.lock().unwrap() = state;
        self.shared.subscribers.lock().unwrap().retain(|s| s.send(line.clone()).is_ok());
        self.last_publish = Instant::now();
    }
}

/// Download a track's audio: radio tracks carry their URL, on-demand ones
/// ask for it. Radio's HE-AAC is decoded here, off the engine thread.
fn fetch(pandora: &Pandora, track: &Value, source: &str) -> anyhow::Result<Loaded> {
    if let Some(url) = track["audioUrl"].as_str().filter(|u| !u.is_empty()) {
        let bytes = pandora.download(url)?;
        let encoding = track["encoding"].as_str().unwrap_or_default();
        let (bytes, encoding) = if encoding == "mp3" { (bytes, "mp3") } else { (decode_with_ffmpeg(&bytes)?, "wav") };
        return Ok(Loaded {
            bytes: Arc::new(bytes),
            encoding: encoding.to_string(),
            gain: track["gain"].as_str().and_then(|g| g.parse().ok()).unwrap_or(0.0),
            receipt: track["receipt"].as_str().unwrap_or_default().to_string(),
        });
    }
    let id = track["id"].as_str().unwrap_or_default();
    let source = if source.is_empty() { id } else { source };
    let info = pandora.call(
        "v1/ondemand/getAudioPlaybackInfo",
        json!({ "pandoraId": id, "sourcePandoraId": source, "deviceCode": "", "includeAudioToken": true }),
    )?;
    let url = info["audioURL"].as_str().ok_or_else(|| anyhow::anyhow!("No audio came back for this track"))?;
    let bytes = pandora.download(url)?;
    Ok(Loaded {
        bytes: Arc::new(bytes),
        encoding: info["audioEncoding"].as_str().unwrap_or("mp3").to_string(),
        gain: info["fileGain"].as_str().and_then(|g| g.parse().ok()).unwrap_or(0.0),
        receipt: info["audioReceiptURL"].as_str().unwrap_or_default().to_string(),
    })
}

/// Where the listener was, kept across restarts and reboots. It holds play
/// tokens, so it is private to the user.
fn resume_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    base.join("omadora").join("resume.json")
}

fn read_resume() -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(resume_path()).ok()?).ok()
}

fn write_resume(state: &Value) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = resume_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(state.to_string().as_bytes()));
    if written.is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
