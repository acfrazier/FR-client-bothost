use std::{env, fs, path::Path};

// Shared with the crate so `resolve_rsa` has one implementation (bake and
// unit tests); it only reads env, never the network.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/login_rsa_resolve.rs"));

/// The bake target: `local` (default) | `live` | `prod`. Cargo reserves
/// the env name `TARGET` for the build script (the compilation target
/// triple, e.g. `aarch64-apple-darwin`), so a shell `TARGET=live` is
/// shadowed — read `BOT_TARGET` as the shadow-resistant spelling, and keep
/// `TARGET` only for contexts where it holds a known campaign value.
fn bake_target() -> String {
    let known = |v: &str| ["local", "live", "prod"].contains(&v);
    env::var("BOT_TARGET")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| env::var("TARGET").ok().filter(|v| known(v)))
        .unwrap_or_else(|| "local".into())
}

fn main() {
    let target = bake_target();
    let (n, e) = resolve_rsa(&target, |k| env::var(k).ok())
        .unwrap_or_else(|err| panic!("{err}"));
    assert!(n.chars().all(|c| c.is_ascii_digit()), "LOGIN_RSAN must be decimal");
    assert!(e.chars().all(|c| c.is_ascii_digit()), "LOGIN_RSAE must be decimal");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("login_rsa_gen.rs");
    fs::write(
        out,
        format!(
            "pub const LOGIN_RSAN: &str = \"{n}\";\npub const LOGIN_RSAE: &str = \"{e}\";\n"
        ),
    )
    .unwrap();
    for var in [
        "TARGET",
        "BOT_TARGET",
        "LIVE_RSAN",
        "PROD_RSAN",
        "LOCAL_RSAN",
        "LOGIN_RSAN",
        "LOGIN_RSAE",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
