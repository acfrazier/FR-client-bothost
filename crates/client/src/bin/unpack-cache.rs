//! `unpack-cache`: one-shot snapshot of the local 274 cache into
//! `~/.274bot/unpack/<version>/`. Reads the versionlist to compute a stable
//! version, unpacks each idx archive into a length-prefixed `.bin`, copies
//! the jag pack verbatim, and prints the manifest. Exits 1 on a real read
//! failure; `size==0` (never-preserved) entries are skipped and counted.

use std::env;
use std::process::ExitCode;

use client::unpack::unpack_cache;

const DEFAULT_CACHE: &str = "experiments/Server/engine/data/pack/client";

struct Args {
    cache: String,
    out: String,
}

fn default_dir(kind: &str) -> String {
    match env::var("HOME") {
        Ok(home) => format!("{home}/{kind}"),
        Err(_) => kind.to_string(),
    }
}

fn parse_args() -> Args {
    let mut args = Args {
        cache: default_dir(DEFAULT_CACHE),
        out: match env::var("HOME") {
            Ok(home) => format!("{home}/.274bot/unpack"),
            Err(_) => ".274bot/unpack".to_string(),
        },
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--cache" => args.cache = value(&mut it),
            "--out" => args.out = value(&mut it),
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }
    args
}

fn value(it: &mut std::iter::Skip<env::Args>) -> String {
    it.next().unwrap_or_else(|| usage())
}

fn usage() -> ! {
    eprintln!("usage: unpack-cache [--cache DIR] [--out DIR]");
    std::process::exit(2);
}

fn main() -> ExitCode {
    let args = parse_args();
    match unpack_cache(&args.cache, &args.out) {
        Ok(m) => {
            println!("version={}", m.version);
            println!("dir={}", m.dir);
            for (name, a) in [
                ("models", &m.models),
                ("anims", &m.anims),
                ("midi", &m.midi),
                ("maps", &m.maps),
            ] {
                println!(
                    "{name}: total={} unpacked={} skipped={}",
                    a.total, a.unpacked, a.skipped
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("unpack-cache: {e}");
            ExitCode::FAILURE
        }
    }
}
