//! Pandora sign-in and the web API.
//!
//! Sign-in goes through the device ("tuner") API, the way pianobar does, with
//! the public Android partner. Its user token is accepted by the web API at
//! www.pandora.com/api, which has the catalog, playlists, and on-demand audio.
//! The web API's own login form sits behind a bot check and is never used.

use anyhow::{Context, Result, anyhow, bail};
use blowfish::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use reqwest::blocking::Client;
use reqwest::cookie::{CookieStore, Jar};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WEB: &str = "https://www.pandora.com";
const TUNER: &str = "https://tuner.pandora.com/services/json/";
pub const USER_AGENT: &str = concat!("Omadora/", env!("CARGO_PKG_VERSION"), " (Linux; Pandora player for Omarchy)");

// pianobar's defaults (src/settings.c): the public Android device partner.
const PARTNER_USER: &str = "android";
const PARTNER_PASSWORD: &str = "AC7IBG09A3DTSYM4R41UJWL07VLN8JI7";
const PARTNER_DEVICE: &str = "android-generic";
const KEY_IN: &[u8] = b"R=U!LH$O2B#";
const KEY_OUT: &[u8] = b"6#26FRL$ZWD";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// The account's email and password. Omadora keeps them in the same file
/// pianobar reads, so both engines sign in with one setup.
pub fn credentials() -> Result<(String, String)> {
    let path = home().join(".config/omadora/pianobar/config");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let (mut user, mut password, mut command) = (None, None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().to_string();
        match key.trim() {
            "user" => user = Some(value),
            "password" => password = Some(value),
            "password_command" => command = Some(value),
            _ => {}
        }
    }
    let password = match (password, command) {
        (Some(p), _) => p,
        (None, Some(cmd)) => {
            let out = Command::new("sh").arg("-c").arg(&cmd).output()?;
            String::from_utf8(out.stdout)?.lines().next().unwrap_or("").to_string()
        }
        (None, None) => bail!("no password in {}", path.display()),
    };
    let user = user.ok_or_else(|| anyhow!("no user in {}", path.display()))?;
    Ok((user, password))
}

fn encrypt(plain: &str) -> String {
    let cipher = <blowfish::Blowfish>::new_from_slice(KEY_OUT).expect("partner key");
    let mut data = plain.as_bytes().to_vec();
    data.resize(data.len().div_ceil(8) * 8, 0);
    for block in data.chunks_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
    hex::encode(data)
}

fn decrypt(hexed: &str) -> Result<Vec<u8>> {
    let cipher = <blowfish::Blowfish>::new_from_slice(KEY_IN).expect("partner key");
    let mut data = hex::decode(hexed)?;
    for block in data.chunks_mut(8) {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    Ok(data)
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// What the account may do, as the device sign-in reports it.
#[derive(Clone, Debug, Default)]
pub struct Account {
    pub subscriber: bool,
    pub audio_ads: bool,
}

/// Pandora treats every sign-in as a separate listener, and only one may
/// play at a time. Keeping the session across engine restarts keeps the
/// engine the same listener. It lives in the runtime dir: private to the
/// user, gone at reboot.
fn session_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime).join("omadora").join("session.json")
}

fn load_session() -> Option<(String, Account)> {
    let saved: Value = serde_json::from_str(&fs::read_to_string(session_path()).ok()?).ok()?;
    let token = saved["token"].as_str().filter(|t| !t.is_empty())?.to_string();
    let account = Account {
        subscriber: saved["subscriber"].as_bool().unwrap_or(false),
        audio_ads: saved["audioAds"].as_bool().unwrap_or(true),
    };
    Some((token, account))
}

fn save_session(token: &str, account: &Account) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let body = json!({ "token": token, "subscriber": account.subscriber, "audioAds": account.audio_ads });
    let tmp = path.with_extension("tmp");
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(body.to_string().as_bytes()));
    if written.is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

/// Partner login, then user login. Returns the user token and the account.
fn device_sign_in(http: &Client) -> Result<(String, Account)> {
    let partner: Value = http
        .post(format!("{TUNER}?method=auth.partnerLogin"))
        .json(&json!({
            "username": PARTNER_USER,
            "password": PARTNER_PASSWORD,
            "deviceModel": PARTNER_DEVICE,
            "version": "5",
            "includeUrls": true,
        }))
        .send()?
        .json()?;
    if partner["stat"] != "ok" {
        bail!("Pandora refused the device sign-in ({})", partner["message"].as_str().unwrap_or("no reason given"));
    }
    let result = &partner["result"];
    let sync = decrypt(result["syncTime"].as_str().unwrap_or_default())?;
    let digits: String = sync.iter().skip(4).map(|b| *b as char).take_while(char::is_ascii_digit).collect();
    let offset = unix_now() - digits.parse::<i64>().context("partner sync time")?;
    let partner_token = result["partnerAuthToken"].as_str().unwrap_or_default().to_string();
    let partner_id = match &result["partnerId"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    let (user, password) = credentials()?;
    let body = json!({
        "loginType": "user",
        "username": user,
        "password": password,
        "partnerAuthToken": partner_token,
        "returnIsSubscriber": true,
        "syncTime": unix_now() - offset,
    });
    let login: Value = http
        .post(format!(
            "{TUNER}?method=auth.userLogin&auth_token={}&partner_id={partner_id}",
            urlencoding::encode(&partner_token)
        ))
        .header("Content-Type", "text/plain")
        .body(encrypt(&body.to_string()))
        .send()?
        .json()?;
    if login["stat"] != "ok" {
        // 1002 is wrong credentials; anything else is Pandora's own message.
        if login["code"] == 1002 {
            bail!("Pandora did not accept the email and password");
        }
        bail!("Pandora refused the sign-in ({})", login["message"].as_str().unwrap_or("no reason given"));
    }
    let result = &login["result"];
    let token = result["userAuthToken"].as_str().ok_or_else(|| anyhow!("sign-in returned no token"))?.to_string();
    let account = Account {
        subscriber: result["isSubscriber"].as_bool().unwrap_or(false),
        audio_ads: result["hasAudioAds"].as_bool().unwrap_or(true),
    };
    Ok((token, account))
}

/// A failed web API call, with Pandora's own reason when it gave one.
#[derive(Debug)]
pub struct PandoraError {
    pub path: String,
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl PandoraError {
    pub fn from_body(path: &str, status: u16, body: &Value) -> Self {
        Self {
            path: path.to_string(),
            status,
            code: body["errorString"].as_str().unwrap_or_default().to_string(),
            message: body["message"].as_str().or(body["errorString"].as_str()).unwrap_or("no details").to_string(),
        }
    }
}

impl std::fmt::Display for PandoraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pandora {} failed: HTTP {}, {}", self.path, self.status, self.message)
    }
}

impl std::error::Error for PandoraError {}

/// A signed-in session on the web API.
pub struct Pandora {
    http: Client,
    csrf: String,
    token: Mutex<String>,
    account: Mutex<Account>,
}

impl Pandora {
    pub fn sign_in() -> Result<Arc<Self>> {
        let jar = Arc::new(Jar::default());
        let http = Client::builder()
            .cookie_provider(jar.clone())
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()?;
        // The web API wants a csrftoken cookie echoed in a header. Any value
        // works; ask for Pandora's own and make one up if none comes back.
        let _ = http.head(WEB).send();
        let url = WEB.parse()?;
        let csrf = jar
            .cookies(&url)
            .and_then(|h| h.to_str().ok().map(str::to_string))
            .and_then(|h| {
                h.split(';')
                    .filter_map(|c| c.trim().split_once('='))
                    .find(|(k, _)| *k == "csrftoken")
                    .map(|(_, v)| v.to_string())
            })
            .unwrap_or_else(|| {
                jar.add_cookie_str("csrftoken=0a1b2c3d4e5f6789", &url);
                "0a1b2c3d4e5f6789".to_string()
            });
        // Reuse the last session if there is one; a stale one is renewed on
        // the first call Pandora rejects.
        let (token, account) = match load_session() {
            Some(saved) => saved,
            None => {
                let fresh = device_sign_in(&http)?;
                save_session(&fresh.0, &fresh.1);
                fresh
            }
        };
        Ok(Arc::new(Self { http, csrf, token: Mutex::new(token), account: Mutex::new(account) }))
    }

    pub fn account(&self) -> Account {
        self.account.lock().unwrap().clone()
    }

    fn renew(&self) -> Result<()> {
        let (token, account) = device_sign_in(&self.http)?;
        save_session(&token, &account);
        *self.token.lock().unwrap() = token;
        *self.account.lock().unwrap() = account;
        Ok(())
    }

    /// POST without retries or error checks: the raw status and body.
    pub fn post(&self, path: &str, body: &Value) -> Result<(u16, Value)> {
        let token = self.token.lock().unwrap().clone();
        let resp = self
            .http
            .post(format!("{WEB}/api/{path}"))
            .header("X-CsrfToken", &self.csrf)
            .header("X-AuthToken", token)
            .json(body)
            .send()?;
        let status = resp.status().as_u16();
        let text = resp.text()?;
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    /// POST to the web API. A rejected token is renewed once and the call retried.
    pub fn call(&self, path: &str, body: Value) -> Result<Value> {
        let (mut status, mut value) = self.post(path, &body)?;
        // A rejected token: 401, or 1001 "Invalid auth token" in the body.
        if status == 401 || value["errorCode"] == 1001 || value["errorString"] == "AUTH_INVALID_TOKEN" {
            self.renew()?;
            (status, value) = self.post(path, &body)?;
        }
        // Some failures come back as HTTP 200 with an errorCode in the body.
        if status != 200 || value.get("errorCode").is_some() {
            return Err(PandoraError::from_body(path, status, &value).into());
        }
        Ok(value)
    }

    /// Download a whole file, such as a track's audio.
    pub fn download(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).timeout(Duration::from_secs(120)).send()?;
        if !resp.status().is_success() {
            bail!("audio download failed: HTTP {}", resp.status());
        }
        Ok(resp.bytes()?.to_vec())
    }

    /// Fire-and-forget GET, for play receipts.
    pub fn ping(&self, url: &str) {
        let _ = self.http.get(url).timeout(Duration::from_secs(15)).send();
    }
}
