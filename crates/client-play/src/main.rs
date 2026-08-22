//! `client-play`: log into a local 274 engine over TCP and run the client
//! machine on the calling thread (`Client::new` + `run`). `--user/--pass`
//! skip title login; without them the title screen is the control plane.
//! `--window` presents the 765×503 applet (feature `window`, highmem);
//! omit it for headless (lowmem, bot-host default). `--lowmem`/`--highmem`
//! override. `--audio` opens the cpal speaker (feature `audio`). A login
//! error (already logged in, wrong password, …) stays in `run` on the
//! title form so Login can be retried.
//!
//! The RSA public half is baked at compile time (`LOGIN_RSAN`/`LOGIN_RSAE`).
//! `tools/redeploy.sh` extracts the engine's `private.pem` and rebuilds this
//! binary with the right key — run the artifact it produced, not a later
//! `cargo run`: a rebuild without those env vars bakes the Java default
//! (wrong) key again.

use std::env;
use std::process::ExitCode;

use client::client::{Client, ClientConfig};

#[cfg(feature = "window")]
use client::client::present::Present;

#[cfg(feature = "audio")]
use client::sound::output::AudioOut;

const DEFAULT_PORT: u16 = 43594;

struct Args {
    host: String,
    port: u16,
    user: String,
    pass: String,
    cache: String,
    window: bool,
    audio: bool,
    /// `None` = pick from `--window` (windowed highmem, headless/bots lowmem).
    lowmem: Option<bool>,
}

fn default_cache_dir() -> String {
    match env::var("HOME") {
        Ok(home) => format!("{home}/experiments/Server/engine/data/pack/client"),
        Err(_) => "experiments/Server/engine/data/pack/client".into(),
    }
}

/// clap-free argv parse: `--key value` pairs plus the `--window`/`--audio`
/// flags. A missing value or an unknown key prints the usage and exits.
/// Credentials are optional: the title screen is the control plane.
fn parse_args() -> Args {
    let mut args = Args {
        host: "127.0.0.1".into(),
        port: DEFAULT_PORT,
        user: String::new(),
        pass: String::new(),
        cache: default_cache_dir(),
        window: false,
        audio: false,
        lowmem: None,
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--host" => args.host = value(&mut it),
            "--port" => args.port = value(&mut it).parse().unwrap_or_else(|_| usage()),
            "--user" => args.user = value(&mut it),
            "--pass" => args.pass = value(&mut it),
            "--cache" => args.cache = value(&mut it),
            "--window" => args.window = true,
            "--audio" => args.audio = true,
            "--lowmem" => args.lowmem = Some(true),
            "--highmem" => args.lowmem = Some(false),
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }
    args
}

fn usage() -> ! {
    eprintln!(
        "usage: client-play [--user USER --pass PASS] \
         [--host HOST] [--port PORT] [--cache DIR] [--window] [--audio] \
         [--lowmem|--highmem]"
    );
    std::process::exit(2);
}

/// Next positional value or usage-exit (`|| usage()` so the never type
/// coerces where a bare `fn() -> !` item does not).
fn value(it: &mut std::iter::Skip<env::Args>) -> String {
    it.next().unwrap_or_else(|| usage())
}

fn main() -> ExitCode {
    let args = parse_args();

    // `--window` without the `window` feature compiled in cannot provide a
    // control plane: refuse to run blind.
    #[cfg(not(feature = "window"))]
    if args.window {
        eprintln!("window: feature not compiled in (build with --features window) - no control plane");
        return ExitCode::FAILURE;
    }

    #[cfg(not(feature = "audio"))]
    if args.audio {
        eprintln!(
            "audio: feature not compiled in (build with --features audio); continuing headless"
        );
    }

    // Windowed play defaults highmem (full textures). Headless / bot-host
    // defaults lowmem. `--lowmem` / `--highmem` override either way. The
    // bot host can also set `ClientConfig.lowmem` directly.
    let lowmem = args.lowmem.unwrap_or(!args.window);
    let config = ClientConfig {
        host: args.host,
        port: args.port,
        cache_dir: args.cache,
        members: true,
        lowmem,
    };
    let mut client = Client::new(config);

    // The 765×503 applet (engine canvas / title.dat). Open failure is fatal
    // (`--window` asked for a control plane); audio failure is not.
    #[cfg(feature = "window")]
    if args.window {
        match Present::open(
            client::client::APPLET_W as u32,
            client::client::APPLET_H as u32,
            "RuneScape",
        ) {
            Ok(present) => {
                client.present = Some(present);
                client.draw = true;
            }
            Err(e) => {
                eprintln!("window: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // On `--window` (or `--audio`) open the speaker: rustysynth render
    // scaled by the shared fade, mixed with the JagFX wave queue. A device
    // failure logs and keeps the picture. The open `AudioOut` must stay
    // alive until after `run`: dropping the cpal `Stream` stops the
    // callback (output.rs), so it is held (unread) to the end of `main`.
    #[cfg(feature = "audio")]
    let mut _audio_out: Option<AudioOut> = None;
    #[cfg(feature = "audio")]
    if args.window || args.audio {
        match AudioOut::try_open(client.midi.clone(), client.waves.clone(), client.fade.clone())
        {
            Ok(out) => {
                eprintln!("audio: speaker {} Hz", out.sample_rate);
                _audio_out = Some(out);
            }
            Err(e) => eprintln!("audio: {e}; continuing without sound"),
        }
    }

    // Jag fetch (`maininit`) runs before login no matter what: the optional
    // `--user/--pass` only skip the title *form* (the username/password
    // fields), not `maininit`. `run`'s guard is then a no-op.
    client.maininit();

    // `--user/--pass` skip title login; without them, run straight to the
    // title screen — it is the control plane (no usage exit).
    if !(args.user.is_empty() || args.pass.is_empty()) {
        match client.login(&args.user, &args.pass, false) {
            Ok(()) => println!("ingame"),
            Err(e) => {
                eprintln!("login {} {} {}", e.code, e.mes1, e.mes2);
                if e.code == 6 {
                    eprintln!("wrong RSA key for this engine - run tools/redeploy.sh and rebuild");
                }
                // Stay in `run` so the title form can retry (window) or the
                // bot host can call `Client::login` again. Do not kill the
                // process on "already logged in" / world-full / etc.
            }
        }
    }

    // Live proof: print the local-player tile every 50 loop_cycle once
    // player info arrives (after REBUILD_NORMAL). `local_player` survives
    // logout Java-shape, so gate on `ingame` — the title screen is not a
    // tile.
    client.run(|c| {
        if c.loop_cycle % 50 == 0 && c.ingame {
            if let Some(p) = &c.local_player {
                println!(
                    "tile: {} {} (cycle {})",
                    c.map_build_base_x + p.route_x[0],
                    c.map_build_base_z + p.route_z[0],
                    c.loop_cycle
                );
            }
        }
    });
    ExitCode::SUCCESS
}
