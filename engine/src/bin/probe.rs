//! Explore Pandora's web API by hand, signed in the way the engine is.
//!
//!   probe call PATH [JSON]   POST JSON to https://www.pandora.com/api/PATH
//!
//! Prints the HTTP status and the response, with tokens, credentials, and
//! personal identifiers redacted. Use it to find endpoints before the engine
//! relies on them.

use anyhow::{Result, anyhow, bail};
use omadora_engine::pandora::Pandora;
use serde_json::{Value, json};

/// Replace secrets and personal identifiers with a marker.
fn redact(value: &Value) -> Value {
    const SECRET: [&str; 9] = ["token", "password", "email", "username", "webname", "birth", "zip", "listenerid", "session"];
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = k.to_lowercase();
                    let secret = SECRET.iter().any(|s| key.contains(s)) && !v.is_object() && !v.is_array();
                    (k.clone(), if secret { json!("<redacted>") } else { redact(v) })
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        Value::String(s) if s.contains("token=") || s.contains("auth=") => {
            json!(format!("{}?<redacted>", s.split('?').next().unwrap_or("")))
        }
        other => other.clone(),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("call") => {
            let path = args.get(1).ok_or_else(|| anyhow!("call PATH [JSON]"))?;
            let body: Value = serde_json::from_str(args.get(2).map(String::as_str).unwrap_or("{}"))?;
            let pandora = Pandora::sign_in()?;
            let (status, value) = pandora.post(path, &body)?;
            println!("HTTP {status}");
            println!("{}", serde_json::to_string_pretty(&redact(&value))?);
            Ok(())
        }
        _ => bail!("usage: probe call PATH [JSON]"),
    }
}
