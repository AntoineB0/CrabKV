use crabkv::CrabKv;
use std::env;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let data_dir = env::var("CRABKV_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));
    let engine = CrabKv::open(&data_dir)?;

    match args.next().as_deref() {
        Some("put") => {
            let key = args
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing key"))?;
            let value = args
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing value"))?;
            engine.put(key, value)?;
            println!("stored");
            Ok(())
        }
        Some("get") => {
            let key = args
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing key"))?;
            match engine.get(&key)? {
                Some(value) => println!("{value}"),
                None => println!("key not found"),
            }
            Ok(())
        }
        Some("delete") => {
            let key = args
                .next()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "missing key"))?;
            engine.delete(&key)?;
            println!("deleted");
            Ok(())
        }
        Some("compact") => {
            engine.compact()?;
            println!("compacted");
            Ok(())
        }
        Some("help") | None => {
            print_usage();
            Ok(())
        }
        Some(cmd) => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unknown command `{cmd}`"),
        )),
    }
}

fn print_usage() {
    println!("CrabKv CLI");
    println!("Usage:");
    println!("  crabkv put <key> <value>");
    println!("  crabkv get <key>");
    println!("  crabkv delete <key>");
    println!("  crabkv compact");
    println!("Set CRABKV_DATA_DIR to override the storage directory.");
}
