use crabkv::{CrabKv, server};
use std::env;
use std::io::{self, ErrorKind};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Ok(());
    }

    let command = args.remove(0);
    let data_dir = data_directory();

    match command.as_str() {
        "put" => cmd_put(&data_dir, args),
        "get" => cmd_get(&data_dir, args),
        "delete" => cmd_delete(&data_dir, args),
        "compact" => cmd_compact(&data_dir, args),
        "serve" => cmd_serve(&data_dir, args),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unknown command `{other}`"),
        )),
    }
}

fn print_usage() {
    println!("CrabKv CLI");
    println!("Usage:");
    println!("  crabkv put <key> <value> [--ttl <seconds>]");
    println!("  crabkv get <key>");
    println!("  crabkv delete <key>");
    println!("  crabkv compact");
    println!("  crabkv serve [--addr <host:port>] [--cache <entries>] [--default-ttl <seconds>]");
    println!(
        "Environment overrides: CRABKV_DATA_DIR, CRABKV_CACHE_CAPACITY, CRABKV_DEFAULT_TTL_SECS"
    );
}

fn cmd_put(data_dir: &Path, mut args: Vec<String>) -> io::Result<()> {
    if args.len() < 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "missing key or value",
        ));
    }
    let key = args.remove(0);
    let value = args.remove(0);
    let ttl = parse_ttl_flag(&args)?;
    let engine = open_engine_with_env(data_dir)?;
    match ttl {
        Some(ttl) => engine.put_with_ttl(key, value, Some(ttl))?,
        None => engine.put(key, value)?,
    }
    println!("stored");
    Ok(())
}

fn cmd_get(data_dir: &Path, mut args: Vec<String>) -> io::Result<()> {
    if args.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "missing key"));
    }
    let key = args.remove(0);
    ensure_no_flags(&args)?;
    let engine = open_engine_with_env(data_dir)?;
    match engine.get(&key)? {
        Some(value) => println!("{value}"),
        None => println!("key not found"),
    }
    Ok(())
}

fn cmd_delete(data_dir: &Path, mut args: Vec<String>) -> io::Result<()> {
    if args.is_empty() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "missing key"));
    }
    let key = args.remove(0);
    ensure_no_flags(&args)?;
    let engine = open_engine_with_env(data_dir)?;
    engine.delete(&key)?;
    println!("deleted");
    Ok(())
}

fn cmd_compact(data_dir: &Path, args: Vec<String>) -> io::Result<()> {
    ensure_no_flags(&args)?;
    let engine = open_engine_with_env(data_dir)?;
    engine.compact()?;
    println!("compacted");
    Ok(())
}

fn cmd_serve(data_dir: &Path, args: Vec<String>) -> io::Result<()> {
    let mut addr = String::from("127.0.0.1:4000");
    let mut cache = env_cache_capacity()?;
    let mut default_ttl = env_default_ttl()?;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--addr" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--addr requires a value")
                })?;
                addr = value.clone();
            }
            "--cache" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--cache requires a value")
                })?;
                cache = parse_cache_capacity(value)?;
            }
            "--default-ttl" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--default-ttl requires a value")
                })?;
                default_ttl = Some(parse_duration_secs(value)?);
            }
            flag => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown option `{flag}`"),
                ));
            }
        }
        index += 1;
    }

    let engine = open_engine(data_dir, cache, default_ttl)?;
    server::run(&addr, engine)
}

fn ensure_no_flags(args: &[String]) -> io::Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    Err(io::Error::new(
        ErrorKind::InvalidInput,
        format!("unexpected arguments: {}", args.join(" ")),
    ))
}

fn parse_ttl_flag(args: &[String]) -> io::Result<Option<Duration>> {
    if args.is_empty() {
        return Ok(None);
    }
    let mut ttl = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--ttl" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "--ttl requires a value")
                })?;
                ttl = Some(parse_duration_secs(value)?);
            }
            flag => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown option `{flag}`"),
                ));
            }
        }
        index += 1;
    }
    Ok(ttl)
}

fn parse_cache_capacity(value: &str) -> io::Result<Option<NonZeroUsize>> {
    let entries: usize = value
        .parse()
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid cache capacity"))?;
    Ok(NonZeroUsize::new(entries))
}

fn parse_duration_secs(input: &str) -> io::Result<Duration> {
    let seconds = input
        .parse::<u64>()
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid seconds"))?;
    Ok(Duration::from_secs(seconds))
}

fn data_directory() -> PathBuf {
    env::var("CRABKV_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"))
}

fn env_cache_capacity() -> io::Result<Option<NonZeroUsize>> {
    match env::var("CRABKV_CACHE_CAPACITY") {
        Ok(value) => parse_cache_capacity(&value),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "invalid CRABKV_CACHE_CAPACITY",
        )),
    }
}

fn env_default_ttl() -> io::Result<Option<Duration>> {
    match env::var("CRABKV_DEFAULT_TTL_SECS") {
        Ok(value) => parse_duration_secs(&value).map(Some),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "invalid CRABKV_DEFAULT_TTL_SECS",
        )),
    }
}

fn open_engine_with_env(data_dir: &Path) -> io::Result<CrabKv> {
    let cache = env_cache_capacity()?;
    let ttl = env_default_ttl()?;
    open_engine(data_dir, cache, ttl)
}

fn open_engine(
    data_dir: &Path,
    cache_capacity: Option<NonZeroUsize>,
    default_ttl: Option<Duration>,
) -> io::Result<CrabKv> {
    let mut builder = CrabKv::builder(data_dir);
    if let Some(capacity) = cache_capacity {
        builder = builder.cache_capacity(capacity);
    }
    if let Some(ttl) = default_ttl {
        builder = builder.default_ttl(ttl);
    }
    builder.build()
}
