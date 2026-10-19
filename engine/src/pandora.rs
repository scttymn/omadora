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

/// The sign-in error when no account is saved; the widget then asks for one.
pub const NO_ACCOUNT: &str = "No account is saved yet";

fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config"));
    base.join("omadora")
}

/// The email of the account signed in from the widget. The password is in
/// the system keyring (libsecret), never in a file.
fn account_path() -> PathBuf {
    config_dir().join("account")
}

fn setting(text: &str, name: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == name)
        .map(|(_, value)| value.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The keyring entry's attributes, as `secret-tool` takes them. Only the
/// application: there's one account, and an email among them would show in
/// the process list while secret-tool runs. (Entries saved with a user
/// attribute still match.)
const KEYRING_ENTRY: [&str; 2] = ["application", "omadora"];

/// The email of the saved account, if there is one.
pub fn saved_user() -> Option<String> {
    fs::read_to_string(account_path()).ok().and_then(|t| setting(&t, "user"))
}

/// The saved account's email and password.
pub fn credentials() -> Result<(String, String)> {
    let Some(user) = saved_user() else { bail!(NO_ACCOUNT) };
    let out = Command::new("secret-tool").arg("lookup").args(KEYRING_ENTRY).output();
    let password = out.ok().and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    if password.is_empty() {
        // The keyring lost it (or is locked); ask again.
        bail!(NO_ACCOUNT);
    }
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
    /// Can play albums, playlists, and songs on demand (Premium). Free
    /// accounts can't; Plus is a subscriber that can't either, which shows
    /// only when Pandora refuses the first on-demand play.
    pub on_demand: bool,
}

/// What the device API needs besides the user token: who the user and the
/// partner are, and how far Pandora's clock is from ours. Kept with the
/// session, so device calls ride on the same sign-in.
#[derive(Clone, Debug, Default)]
pub struct Device {
    pub user_id: String,
    pub partner_id: String,
    pub offset: i64,
}

/// Keep a newly accepted account: the password in the keyring, the email in
/// the account file. Any account saved before is replaced.
fn remember(user: &str, password: &str) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let _ = Command::new("secret-tool").args(["clear", "application", "omadora"]).output();
    let mut child = Command::new("secret-tool")
        .args(["store", "--label=Omadora account"])
        .args(KEYRING_ENTRY)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("saving the password needs secret-tool (libsecret)")?;
    child.stdin.take().context("secret-tool's input")?.write_all(password.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("the keyring did not save the password ({})", String::from_utf8_lossy(&out.stderr).trim());
    }
    let path = account_path();
    crate::files::write_private(&path, format!("user = {user}\n").as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Sign out: forget the saved account and the session.
pub fn forget() {
    let _ = Command::new("secret-tool").args(["clear", "application", "omadora"]).output();
    let _ = fs::remove_file(account_path());
    if let Some(path) = session_path() {
        let _ = fs::remove_file(path);
    }
}

/// Pandora treats every sign-in as a separate listener, and only one may
/// play at a time. Keeping the session across engine restarts keeps the
/// engine the same listener. It lives in the runtime dir: private to the
/// user, gone at reboot.
fn session_path() -> Option<PathBuf> {
    crate::files::runtime_dir().ok().map(|dir| dir.join("session.json"))
}

fn load_session() -> Option<(String, Account, Option<Device>)> {
    let saved: Value = serde_json::from_str(&fs::read_to_string(session_path()?).ok()?).ok()?;
    let token = saved["token"].as_str().filter(|t| !t.is_empty())?.to_string();
    let subscriber = saved["subscriber"].as_bool().unwrap_or(false);
    let account = Account {
        subscriber,
        audio_ads: saved["audioAds"].as_bool().unwrap_or(true),
        on_demand: saved["onDemand"].as_bool().unwrap_or(subscriber),
    };
    let device = saved["userId"].as_str().filter(|u| !u.is_empty()).map(|user_id| Device {
        user_id: user_id.to_string(),
        partner_id: saved["partnerId"].as_str().unwrap_or_default().to_string(),
        offset: saved["syncOffset"].as_i64().unwrap_or(0),
    });
    Some((token, account, device))
}

fn save_session(token: &str, account: &Account, device: Option<&Device>) {
    let Some(path) = session_path() else { return };
    let mut body = json!({ "token": token, "subscriber": account.subscriber, "audioAds": account.audio_ads, "onDemand": account.on_demand });
    if let Some(d) = device {
        body["userId"] = json!(d.user_id);
        body["partnerId"] = json!(d.partner_id);
        body["syncOffset"] = json!(d.offset);
    }
    let _ = crate::files::write_private(&path, body.to_string().as_bytes());
}

/// Partner login, then user login. Returns the user token and the account.
fn device_sign_in(http: &Client, user: &str, password: &str) -> Result<(String, Account, Device)> {
    let partner: Value = http
        .post(format!("{TUNER}?method=auth.partnerLogin"))
        .json(&json!({
            "username": PARTNER_USER,
            "password": PARTNER_PASSWORD,
            "deviceModel": PARTNER_DEVICE,
            "version": "5",
            "includeUrls": true,
        }))
        .send()
        .map_err(reqwest::Error::without_url)?
        .json()?;
    if partner["stat"] != "ok" {
        bail!("The device sign-in was refused ({})", partner["message"].as_str().unwrap_or("no reason given"));
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
        .send()
        .map_err(reqwest::Error::without_url)?
        .json()?;
    if login["stat"] != "ok" {
        // 1002 is wrong credentials; anything else is Pandora's own message.
        if login["code"] == 1002 {
            bail!("That email and password didn't work.");
        }
        bail!("The sign-in was refused ({})", login["message"].as_str().unwrap_or("no reason given"));
    }
    let result = &login["result"];
    let token = result["userAuthToken"].as_str().ok_or_else(|| anyhow!("sign-in returned no token"))?.to_string();
    let subscriber = result["isSubscriber"].as_bool().unwrap_or(false);
    let account = Account {
        subscriber,
        audio_ads: result["hasAudioAds"].as_bool().unwrap_or(true),
        on_demand: subscriber,
    };
    let user_id = match &result["userId"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Ok((token, account, Device { user_id, partner_id, offset }))
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
        write!(f, "{} failed: HTTP {}, {}", self.path, self.status, self.message)
    }
}

impl std::error::Error for PandoraError {}

/// A signed-in session on the web API.
pub struct Pandora {
    http: Client,
    csrf: String,
    token: Mutex<String>,
    account: Mutex<Account>,
    device: Mutex<Option<Device>>,
}

impl Pandora {
    /// Sign in with the saved account, reusing the last session if there is one.
    pub fn sign_in() -> Result<Arc<Self>> {
        Self::connect(None)
    }

    /// Sign in with an email and password from the widget, and save them once
    /// Pandora accepts them.
    pub fn sign_in_with(user: &str, password: &str) -> Result<Arc<Self>> {
        Self::connect(Some((user, password)))
    }

    fn connect(given: Option<(&str, &str)>) -> Result<Arc<Self>> {
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
        let (token, account, device) = match (given, load_session()) {
            (Some((user, password)), _) => {
                let (token, account, device) = device_sign_in(&http, user, password)?;
                remember(user, password)?;
                save_session(&token, &account, Some(&device));
                (token, account, Some(device))
            }
            (None, Some(saved)) => saved,
            (None, None) => {
                let (user, password) = credentials()?;
                let (token, account, device) = device_sign_in(&http, &user, &password)?;
                save_session(&token, &account, Some(&device));
                (token, account, Some(device))
            }
        };
        Ok(Arc::new(Self { http, csrf, token: Mutex::new(token), account: Mutex::new(account), device: Mutex::new(device) }))
    }

    pub fn account(&self) -> Account {
        self.account.lock().unwrap().clone()
    }

    /// Pandora refused an on-demand play: this account plays radio only.
    /// Remembered with the session.
    pub fn radio_only(&self) {
        let mut account = self.account.lock().unwrap();
        account.on_demand = false;
        save_session(&self.token.lock().unwrap(), &account, self.device.lock().unwrap().as_ref());
    }

    fn renew(&self) -> Result<()> {
        let (user, password) = credentials()?;
        let (token, account, device) = device_sign_in(&self.http, &user, &password)?;
        save_session(&token, &account, Some(&device));
        *self.token.lock().unwrap() = token;
        *self.account.lock().unwrap() = account;
        *self.device.lock().unwrap() = Some(device);
        Ok(())
    }

    /// A call to the device ("tuner") API, which carries Pandora Free's
    /// audio ads: the body encrypted, signed with the user token and
    /// Pandora's clock. A rejected token is renewed once and the call
    /// retried; a session from before this was kept is renewed first.
    pub fn device_call(&self, method: &str, body: Value) -> Result<Value> {
        if self.device.lock().unwrap().is_none() {
            self.renew()?;
        }
        let attempt = |body: &Value| -> Result<Value> {
            let device = self.device.lock().unwrap().clone().unwrap_or_default();
            let token = self.token.lock().unwrap().clone();
            let mut body = body.clone();
            body["userAuthToken"] = json!(token);
            body["syncTime"] = json!(unix_now() - device.offset);
            let reply: Value = self
                .http
                .post(format!(
                    "{TUNER}?method={method}&auth_token={}&partner_id={}&user_id={}",
                    urlencoding::encode(&token),
                    device.partner_id,
                    device.user_id
                ))
                .header("Content-Type", "text/plain")
                .body(encrypt(&body.to_string()))
                .send()
                .map_err(reqwest::Error::without_url)?
                .json()?;
            Ok(reply)
        };
        let mut reply = attempt(&body)?;
        // 1001: the token has expired.
        if reply["stat"] != "ok" && reply["code"] == 1001 {
            self.renew()?;
            reply = attempt(&body)?;
        }
        if reply["stat"] != "ok" {
            bail!("{method} failed ({}: {})", reply["code"], reply["message"].as_str().unwrap_or("no reason given"));
        }
        Ok(reply["result"].clone())
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
            .send()
            .map_err(reqwest::Error::without_url)?;
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
        let resp = self.http.get(url).timeout(Duration::from_secs(120)).send().map_err(reqwest::Error::without_url)?;
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
