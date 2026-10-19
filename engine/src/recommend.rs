//! The AI tab: songs and artists like the one playing, suggested by an AI agent and
//! checked against the catalog, so every suggestion plays.
//!
//! The agent is the one in `~/.config/omadora/settings.toml`, else Omarchy's
//! default agent (`omarchy-default-agent`):
//!
//! ```toml
//! [ai]
//! agent = "claude"        # one of AGENTS; overrides Omarchy's default
//! command = "my-llm"      # or any command: prompt on stdin, answer on stdout
//! prompt = "Suggest deep cuts from the same scene and decade."
//! ```
//!
//! `prompt` replaces DEFAULT_PROMPT, the part that says what to suggest.
//!
//! Agents run headless with no tools: all that is wanted is a short JSON
//! answer. Answers are kept per song, so asking again costs nothing.

use crate::discover::{Lead, resolve};
use crate::pandora::Pandora;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long an agent may think before Omadora gives up on it.
const TIMEOUT: Duration = Duration::from_secs(180);
/// The most suggestions the tab shows, whatever the prompt asks for.
const PICKS: usize = 8;

/// Agents Omadora knows how to ask, with the names the tab shows. The same
/// set Omarchy can make the default.
pub const AGENTS: [(&str, &str); 9] = [
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("gemini", "Gemini"),
    ("opencode", "OpenCode"),
    ("copilot", "GitHub Copilot"),
    ("crush", "Crush"),
    ("grok", "Grok"),
    ("pi", "Pi"),
    ("omp", "Oh My Pi"),
];

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn settings_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config"));
    base.join("omadora").join("settings.toml")
}

fn settings() -> toml::Table {
    std::fs::read_to_string(settings_path()).ok().and_then(|t| t.parse().ok()).unwrap_or_default()
}

fn ai_setting(key: &str) -> Option<String> {
    settings()
        .get("ai")
        .and_then(|ai| ai.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn installed(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg("command -v \"$1\" >/dev/null 2>&1")
        .arg("sh")
        .arg(command)
        .status()
        .is_ok_and(|s| s.success())
}

fn display_name(agent: &str) -> String {
    AGENTS.iter().find(|(id, _)| *id == agent).map_or_else(|| agent.to_string(), |(_, name)| name.to_string())
}

/// Which agent answers, and where that choice came from.
struct Choice {
    agent: String,
    from: &'static str,
}

fn choice() -> Option<Choice> {
    if ai_setting("command").is_some() {
        return Some(Choice { agent: "command".into(), from: "omadora" });
    }
    if let Some(agent) = ai_setting("agent") {
        return Some(Choice { agent, from: "omadora" });
    }
    let out = Command::new("omarchy-default-agent").output().ok()?;
    let agent = String::from_utf8_lossy(&out.stdout).split_whitespace().next()?.to_string();
    Some(Choice { agent, from: "omarchy" })
}

/// The agent in use and the ones that could be: for the tab's picker.
pub fn agents() -> Value {
    let current = choice();
    json!({
        "agent": current.as_ref().map(|c| c.agent.clone()),
        "name": current.as_ref().map(|c| if c.agent == "command" { "your command".to_string() } else { display_name(&c.agent) }),
        "from": current.as_ref().map(|c| c.from),
        "installed": AGENTS
            .iter()
            .filter(|(id, _)| installed(id))
            .map(|(id, name)| json!({ "id": id, "name": name }))
            .collect::<Vec<_>>(),
    })
}

/// Save Omadora's own agent, keeping the rest of the settings file.
pub fn set_agent(agent: &str) -> Result<Value> {
    if !AGENTS.iter().any(|(id, _)| *id == agent) {
        bail!("Omadora doesn't know how to ask {agent}.");
    }
    let mut all = settings();
    let ai = all.entry("ai").or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(ai) = ai else { bail!("[ai] in {} is not a table", settings_path().display()) };
    ai.insert("agent".into(), toml::Value::String(agent.into()));
    let path = settings_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, toml::to_string(&all)?).with_context(|| format!("writing {}", path.display()))?;
    CACHE.lock().unwrap().clear();
    Ok(agents())
}

/// The command line for one headless question. Codex writes its final
/// message to `answer`; the others print it.
fn argv(agent: &str, prompt: &str, answer: &std::path::Path) -> Option<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut args = match agent {
        "claude" => s(&["claude", "-p", "--output-format", "text", "--disallowedTools", "Bash", "Edit", "Write", "WebFetch", "WebSearch", "--"]),
        "codex" => s(&["codex", "exec", "--skip-git-repo-check", "--ephemeral", "--sandbox", "read-only", "--color", "never", "-o"]),
        "gemini" => s(&["gemini", "-p"]),
        "opencode" => s(&["opencode", "run"]),
        "copilot" => s(&["copilot", "-p"]),
        "crush" => s(&["crush", "run"]),
        "grok" => s(&["grok", "-p"]),
        "pi" => s(&["pi", "-p"]),
        "omp" => s(&["omp", "-p"]),
        _ => return None,
    };
    if agent == "codex" {
        args.push(answer.to_string_lossy().into_owned());
    }
    args.push(prompt.to_string());
    Some(args)
}

/// Run a command to completion or the timeout; its stdout.
fn run(mut command: Command, input: Option<&str>) -> Result<String> {
    let dir = std::env::temp_dir();
    command.current_dir(&dir).stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(text.as_bytes())?;
    }
    // Drain both pipes on their own threads, so a chatty agent can't block.
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("The agent took longer than {} seconds.", TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    if !status.success() {
        let reason = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim().to_string();
        bail!("The agent stopped with an error ({reason}).");
    }
    Ok(stdout)
}

/// The JSON object with "picks" somewhere in an agent's reply, which may
/// come wrapped in prose or a code fence.
fn picks_from(reply: &str) -> Option<Vec<Value>> {
    let mut start = 0;
    while let Some(i) = reply[start..].find('{') {
        let from = start + i;
        let mut stream = serde_json::Deserializer::from_str(&reply[from..]).into_iter::<Value>();
        if let Some(Ok(value)) = stream.next() {
            if let Some(picks) = value["picks"].as_array() {
                return Some(picks.clone());
            }
        }
        start = from + 1;
    }
    None
}

/// What to suggest and how: the part of the prompt a listener can replace
/// with `[ai] prompt` in settings.toml. What's playing and the answer's
/// format are always added around it.
pub const DEFAULT_PROMPT: &str = "Suggest 8 things they would enjoy next: about 5 similar songs and 3 similar artists. \
Songs: real, released original recordings likely to be on major streaming services, never remixes, live versions, \
edits, or covers; mostly other artists, at most one by the artist playing; varied, not only the biggest hits; \
nothing they just played. Artists: other artists they would like. \
For each, \"why\" is one short sentence (under 15 words) on what connects it to what they're playing.";

fn prompt(state: &Value) -> String {
    let track = &state["track"];
    let line = |t: &Value| format!("\"{}\" by {}", t["title"].as_str().unwrap_or_default(), t["artist"].as_str().unwrap_or_default());
    let history = state["history"].as_array().cloned().unwrap_or_default();
    let recent: Vec<String> = history.iter().rev().take(10).map(line).collect();
    let liked: Vec<String> = history.iter().filter(|t| t["rating"] == 1).map(line).collect();
    let mut p = format!(
        "You suggest music inside a music player. The listener is playing {} from the album \"{}\".\n",
        line(track),
        track["album"].as_str().unwrap_or_default()
    );
    if !recent.is_empty() {
        p += &format!("Recently played: {}.\n", recent.join("; "));
    }
    if !liked.is_empty() {
        p += &format!("They gave a thumbs up to: {}.\n", liked.join("; "));
    }
    p += &ai_setting("prompt").unwrap_or_else(|| DEFAULT_PROMPT.to_string());
    p += &format!(
        "\nAnswer from what you know; don't use tools or the web. Give at most {PICKS} picks. For each: the artist, \
         the song title exactly as released (empty when suggesting the artist), and why. \
         Reply with only this JSON and nothing else: \
         {{\"picks\":[{{\"artist\":\"...\",\"song\":\"...\",\"why\":\"...\"}}]}}"
    );
    p
}

/// Answers by song, for this run of the engine.
static CACHE: LazyLock<Mutex<HashMap<String, Value>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// One question at a time; a second ask for the same song waits and then
/// finds the first one's answer.
static ASKING: Mutex<()> = Mutex::new(());

/// Songs like the one playing. `state` is the engine's published state.
pub fn recommend(pandora: &Pandora, state: &Value, fresh: bool) -> Result<Value> {
    let track = &state["track"];
    let key = track["id"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("Nothing is playing."))?.to_string();
    let _turn = ASKING.lock().unwrap_or_else(|e| e.into_inner());
    if !fresh {
        if let Some(found) = CACHE.lock().unwrap().get(&key) {
            return Ok(found.clone());
        }
    }
    let Some(choice) = choice() else { bail!("No agent is set.") };
    let name = if choice.agent == "command" { "your command".to_string() } else { display_name(&choice.agent) };
    let question = prompt(state);
    let answer_file = std::env::temp_dir().join(format!("omadora-answer-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&answer_file);
    let reply = if let Some(command) = ai_setting("command") {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        run(c, Some(&question))?
    } else {
        let args = argv(&choice.agent, &question, &answer_file).ok_or_else(|| anyhow!("Omadora doesn't know how to ask {name} yet."))?;
        if !installed(&args[0]) {
            bail!("{name} isn't installed.");
        }
        let mut c = Command::new(&args[0]);
        c.args(&args[1..]);
        let printed = run(c, None)?;
        std::fs::read_to_string(&answer_file).ok().filter(|s| !s.trim().is_empty()).unwrap_or(printed)
    };
    let _ = std::fs::remove_file(&answer_file);
    let picks = picks_from(&reply).ok_or_else(|| anyhow!("{name} didn't answer with suggestions."))?;

    // Check each against the catalog at once: the song if it is there, else
    // the artist, else nothing.
    let checked: Vec<Option<Value>> = std::thread::scope(|scope| {
        let handles: Vec<_> = picks
            .iter()
            .take(PICKS + 4)
            .map(|pick| {
                scope.spawn(move || {
                    let artist = pick["artist"].as_str().unwrap_or_default().trim().to_string();
                    let song = pick["song"].as_str().unwrap_or_default().trim().to_string();
                    if artist.is_empty() {
                        return None;
                    }
                    let found = (!song.is_empty())
                        .then(|| resolve(pandora, &Lead::Track { title: song, artist: artist.clone() }))
                        .flatten()
                        .or_else(|| resolve(pandora, &Lead::Artist { name: artist }))?;
                    Some(json!({ "why": pick["why"].as_str().unwrap_or_default().trim(), "item": found }))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    let mut seen = std::collections::HashSet::new();
    let found: Vec<Value> = checked
        .into_iter()
        .flatten()
        .filter(|p| p["item"]["id"].as_str().is_some_and(|id| id != key && seen.insert(id.to_string())))
        .take(PICKS)
        .collect();
    let result = json!({
        "for": { "id": key, "title": track["title"], "artist": track["artist"] },
        "agent": name,
        "picks": found,
        "missing": picks.len().saturating_sub(found.len()),
    });
    CACHE.lock().unwrap().insert(key, result.clone());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_always_has_the_song_and_the_answer_format() {
        let state = json!({ "track": { "title": "Shout", "artist": "Tears For Fears", "album": "Songs From The Big Chair" } });
        let p = prompt(&state);
        assert!(p.contains("\"Shout\" by Tears For Fears"));
        assert!(p.contains("\"picks\""));
    }

    #[test]
    fn picks_are_found_inside_prose_and_fences() {
        let reply = "Sure! Here you go:\n```json\n{\"picks\":[{\"artist\":\"A\",\"song\":\"B\",\"why\":\"C\"}]}\n```\nEnjoy {not json}.";
        let picks = picks_from(reply).unwrap();
        assert_eq!(picks[0]["artist"], "A");
        assert!(picks_from("no json here {").is_none());
    }

    #[test]
    fn every_agent_puts_the_prompt_last() {
        let answer = std::path::Path::new("/tmp/a.txt");
        for (id, _) in AGENTS {
            let args = argv(id, "PROMPT", answer).unwrap();
            assert_eq!(args[0], id);
            assert_eq!(args.last().unwrap(), "PROMPT");
        }
        assert!(argv("muse", "PROMPT", answer).is_none());
    }
}
