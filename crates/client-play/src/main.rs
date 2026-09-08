//! `client-play`: log into a local engine over TCP and run the client machine
//! on the calling thread. Revision 274 is the default; pass `--revision 289`
//! to opt into the source-grounded 289 profile. `--user/--pass`
//! skip title login; without them the title screen is the control plane.
//! `--window` presents the 765×503 applet (feature `window`, highmem);
//! omit it for headless (lowmem, bot-host default). `--lowmem`/`--highmem`
//! override. `--audio` opens the cpal speaker (feature `audio`).
//! `--http-port N` points the jag-fetch web calls (the `/crc` table and the
//! jag GETs) at port N instead of the default 80. A login
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

use client::client::{Client, ClientConfig, ClientRevision};
use client::render::Renderer;

#[cfg(feature = "window")]
use client::client::present::WindowTarget;

#[cfg(feature = "audio")]
use client::sound::output::AudioOut;

const DEFAULT_PORT: u16 = 43594;

struct Args {
    host: String,
    port: u16,
    /// Jag-fetch web port (`Client.http_port`): the web-origin port the
    /// `maininit` HTTP fetch (`/crc`, the jag GETs) hits. Default 80; a
    /// non-privileged local engine web server is reachable with
    /// `--http-port N`.
    http_port: u16,
    user: String,
    pass: String,
    cache: String,
    window: bool,
    audio: bool,
    /// Force the CPU backend even with `--window` (visual fidelity check).
    cpu: bool,
    /// `None` = pick from `--window` (windowed highmem, headless/bots lowmem).
    lowmem: Option<bool>,
    revision: ClientRevision,
}

fn default_cache_dir() -> String {
    client::cache_dir().display().to_string()
}

/// clap-free argv parse: `--key value` pairs plus the `--window`/`--audio`
/// flags. A missing value or an unknown key prints the usage and exits.
/// Credentials are optional: the title screen is the control plane.
fn parse_args_from<I>(values: I) -> Result<Args, ()>
where
    I: IntoIterator<Item = String>,
{
    let mut args = Args {
        host: "127.0.0.1".into(),
        port: DEFAULT_PORT,
        http_port: 80,
        user: String::new(),
        pass: String::new(),
        cache: String::new(),
        window: false,
        audio: false,
        cpu: false,
        lowmem: None,
        revision: ClientRevision::R274,
    };
    let mut it = values.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--host" => args.host = value(&mut it).ok_or(())?,
            "--port" => args.port = value(&mut it).ok_or(())?.parse().map_err(|_| ())?,
            "--http-port" => args.http_port = value(&mut it).ok_or(())?.parse().map_err(|_| ())?,
            "--user" => args.user = value(&mut it).ok_or(())?,
            "--pass" => args.pass = value(&mut it).ok_or(())?,
            "--cache" => args.cache = value(&mut it).ok_or(())?,
            "--revision" => {
                args.revision = match value(&mut it).ok_or(())?.as_str() {
                    "274" => ClientRevision::R274,
                    "289" => ClientRevision::R289,
                    _ => return Err(()),
                };
            }
            "--window" => args.window = true,
            "--audio" => args.audio = true,
            "--cpu" => args.cpu = true,
            "--lowmem" => args.lowmem = Some(true),
            "--highmem" => args.lowmem = Some(false),
            "--help" | "-h" => return Err(()),
            _ => return Err(()),
        }
    }
    if args.cache.is_empty() {
        args.cache = default_cache_dir();
    }
    Ok(args)
}

fn parse_args() -> Args {
    parse_args_from(env::args().skip(1)).unwrap_or_else(|_| usage())
}

fn usage() -> ! {
    eprintln!(
        "usage: client-play [--user USER --pass PASS] \
         [--host HOST] [--port PORT] [--http-port PORT] [--cache DIR] \
         [--revision 274|289] \
         [--window] [--audio] [--cpu] [--lowmem|--highmem]"
    );
    std::process::exit(2);
}

/// Next positional value, if one is present.
fn value<I>(it: &mut I) -> Option<String>
where
    I: Iterator<Item = String>,
{
    it.next()
}

fn main() -> ExitCode {
    let args = parse_args();

    // `--window` without the `window` feature compiled in cannot provide a
    // control plane: refuse to run blind.
    #[cfg(not(feature = "window"))]
    if args.window {
        eprintln!(
            "window: feature not compiled in (build with --features window) - no control plane"
        );
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
    // Bind the web-origin port before construction: standalone local engines
    // commonly use a non-privileged port instead of the default port 80.
    let mut client = Client::new_with_revision_and_http_port(
        config,
        args.revision,
        args.http_port,
    );
    // Render state is separate (task 2b): the driver holds the `Renderer`
    // beside the sim `Client` and hands it to `maininit`/`run`.
    // `--window` prefers the wgpu backend (task 7): the 3D scene is
    // rasterized on the GPU where an adapter exists, else the renderer
    // falls back to the CPU backend (logged, never fatal). Headless /
    // bot-host stays CPU (the fidelity path).
    Renderer::set_prefer_gpu(args.window && !args.cpu);
    let mut renderer = Renderer::new(lowmem);
    eprintln!(
        "render backend: {}",
        match renderer.backend_kind() {
            client::render::backend::BackendKind::Gpu => "wgpu",
            client::render::backend::BackendKind::Cpu => "cpu",
        }
    );

    // The 765×503 applet (engine canvas / title.dat). Open failure is fatal
    // (`--window` asked for a control plane); audio failure is not.
    #[cfg(feature = "window")]
    if args.window {
        match WindowTarget::open(
            client::client::APPLET_W as u32,
            client::client::APPLET_H as u32,
            "RuneScape",
        ) {
            Ok(window) => {
                client.present = Some(Box::new(window));
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
        match AudioOut::try_open(
            client.midi.clone(),
            client.waves.clone(),
            client.fade.clone(),
        ) {
            Ok(out) => {
                eprintln!("audio: speaker {} Hz", out.sample_rate);
                _audio_out = Some(out);
            }
            Err(e) => eprintln!("audio: {e}; continuing without sound"),
        }
    }

    // Jag fetch (`maininit`) runs before login no matter what: the optional
    // `--user/--pass` only skip the title *form* (the username/password
    // fields), not `maininit`. `run`'s guard is then a no-op. The progress
    // callback keeps the loading bar drawing synchronously during the
    // renderer-free load (maininit itself names no `Renderer`).
    client.maininit_with_progress(Some(&mut |c, m, p| renderer.draw_progress(c, m, p)));

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
    client.run(&mut renderer, |c| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn revision_defaults_to_274() {
        assert_eq!(
            parse_args_from(Vec::new()).unwrap().revision,
            ClientRevision::R274
        );
    }

    #[test]
    fn revision_accepts_289_and_preserves_other_options() {
        let args = parse_args_from(argv(&["--revision", "289", "--port", "1234"])).unwrap();
        assert_eq!(args.revision, ClientRevision::R289);
        assert_eq!(args.port, 1234);
    }

    #[test]
    fn revision_rejects_invalid_and_missing_values() {
        assert!(parse_args_from(argv(&["--revision", "290"])).is_err());
        assert!(parse_args_from(argv(&["--revision"])).is_err());
    }

    #[test]
    fn selected_revision_reaches_client_construction() {
        let config = ClientConfig {
            host: "127.0.0.1".into(),
            port: DEFAULT_PORT,
            cache_dir: "/path/that-does-not-exist".into(),
            members: true,
            lowmem: true,
        };
        let client = Client::from_shared_with_revision(
            config,
            Arc::new(client::config::Cache::default()),
            Arc::new(Vec::new()),
            Arc::new(Vec::new()),
            ClientRevision::R289,
        );
        assert_eq!(client.revision(), ClientRevision::R289);
    }
}
