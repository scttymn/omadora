//! omadora-engine            run the engine (the widget starts this)
//! omadora-engine send JSON  send one request and print what comes back
//!     (JSON may be "-" to read it from stdin, as for signIn)
//!     --events N            also print state events for N seconds

use anyhow::{Result, bail};
use omadora_engine::{engine, server};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => {
            let shared = engine::start()?;
            server::serve(shared)
        }
        Some("send") => send(&args[1..]),
        Some(other) => bail!("unknown command {other}; use `serve` or `send JSON`"),
    }
}

fn send(args: &[String]) -> Result<()> {
    let Some(raw) = args.first() else { bail!("send JSON [--events SECONDS]") };
    // "-" reads the request from stdin, keeping a password off the command line.
    let raw = if raw == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
        text
    } else {
        raw.clone()
    };
    let mut request: Value = serde_json::from_str(&raw)?;
    if request.get("req").is_none() {
        request["req"] = Value::from(1);
    }
    let events_for = match args.iter().position(|a| a == "--events") {
        Some(i) => Duration::from_secs_f64(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5.0)),
        None => Duration::ZERO,
    };
    let mut stream = UnixStream::connect(server::socket_path()?)?;
    stream.write_all(format!("{request}\n").as_bytes())?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    let started = Instant::now();
    let mut answered = false;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while started.elapsed() < Duration::from_secs(60).max(events_for) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let msg: Value = serde_json::from_str(&line)?;
                if msg.get("event").is_some() {
                    if !events_for.is_zero() {
                        println!("{}", line.trim_end());
                    }
                } else if msg["req"] == request["req"] {
                    println!("{}", line.trim_end());
                    answered = true;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
        if answered && started.elapsed() >= events_for {
            break;
        }
    }
    Ok(())
}
