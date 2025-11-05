//! Minimal TCP front-end exposing the CrabKv API.

use crate::engine::CrabKv;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

const HELP: &str =
    "Commands: PUT <key> <value> [ttl=<seconds>], GET <key>, DELETE <key>, COMPACT, HELP";

/// Starts a blocking TCP server handling text commands.
pub fn run(addr: &str, engine: CrabKv) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("CrabKv TCP server listening on {addr}");
    for stream in listener.incoming() {
        let stream = stream?;
        let engine = engine.clone();
        thread::spawn(move || {
            if let Err(err) = handle_client(stream, engine) {
                eprintln!("client error: {err}");
            }
        });
    }
    Ok(())
}

fn handle_client(stream: TcpStream, engine: CrabKv) -> io::Result<()> {
    let peer = stream.peer_addr().ok();
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    writeln!(writer, "Welcome to CrabKv. {HELP}")?;

    for line in reader.lines() {
        let line = line?;
        let response = match parse_command(&line) {
            Command::Put { key, value, ttl } => match ttl {
                Some(ttl) => engine
                    .put_with_ttl(key, value, Some(ttl))
                    .map(|_| "OK".to_string()),
                None => engine.put(key, value).map(|_| "OK".to_string()),
            },
            Command::Get { key } => match engine.get(&key)? {
                Some(value) => Ok(format!("VALUE {value}")),
                None => Ok("NOT_FOUND".to_string()),
            },
            Command::Delete { key } => engine.delete(&key).map(|_| "OK".to_string()),
            Command::Compact => engine.compact().map(|_| "OK".to_string()),
            Command::Help => Ok(HELP.to_string()),
            Command::Invalid => Err(io::Error::new(io::ErrorKind::InvalidInput, "bad command")),
        };

        match response {
            Ok(output) => {
                writeln!(writer, "{output}")?;
            }
            Err(err) => {
                writeln!(writer, "ERR {err}")?;
            }
        }
        writer.flush()?;
    }

    if let Some(addr) = peer {
        println!("connection closed: {addr}");
    }
    Ok(())
}

enum Command {
    Put {
        key: String,
        value: String,
        ttl: Option<Duration>,
    },
    Get {
        key: String,
    },
    Delete {
        key: String,
    },
    Compact,
    Help,
    Invalid,
}

fn parse_command(line: &str) -> Command {
    let mut parts = line.trim().split_whitespace();
    match parts.next() {
        Some(cmd) if cmd.eq_ignore_ascii_case("put") => {
            let key = match parts.next() {
                Some(key) => key.to_owned(),
                None => return Command::Invalid,
            };
            let value = match parts.next() {
                Some(value) => value.to_owned(),
                None => return Command::Invalid,
            };
            let ttl = parts.next().and_then(parse_ttl_kv);
            if parts.next().is_some() {
                return Command::Invalid;
            }
            Command::Put { key, value, ttl }
        }
        Some(cmd) if cmd.eq_ignore_ascii_case("get") => match parts.next() {
            Some(key) => {
                if parts.next().is_some() {
                    Command::Invalid
                } else {
                    Command::Get {
                        key: key.to_owned(),
                    }
                }
            }
            None => Command::Invalid,
        },
        Some(cmd) if cmd.eq_ignore_ascii_case("delete") => match parts.next() {
            Some(key) => {
                if parts.next().is_some() {
                    Command::Invalid
                } else {
                    Command::Delete {
                        key: key.to_owned(),
                    }
                }
            }
            None => Command::Invalid,
        },
        Some(cmd) if cmd.eq_ignore_ascii_case("compact") => {
            if parts.next().is_some() {
                Command::Invalid
            } else {
                Command::Compact
            }
        }
        Some(cmd) if cmd.eq_ignore_ascii_case("help") => {
            if parts.next().is_some() {
                Command::Invalid
            } else {
                Command::Help
            }
        }
        _ => Command::Invalid,
    }
}

fn parse_ttl_kv(token: &str) -> Option<Duration> {
    let (key, value) = token.split_once('=')?;
    if key.eq_ignore_ascii_case("ttl") {
        parse_duration_secs(value).ok()
    } else {
        None
    }
}

fn parse_duration_secs(input: &str) -> io::Result<Duration> {
    let seconds = u64::from_str(input)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid TTL"))?;
    Ok(Duration::from_secs(seconds))
}
