pub mod client;
pub mod config;
pub mod core;
pub mod dash3d;
pub mod datastruct;
pub mod graphics;
pub mod io;
pub mod login_rsa;
pub mod render;
pub mod sound;
pub mod unpack;
pub mod util;
pub mod wordfilter;
pub use login_rsa::{LOGIN_RSAE, LOGIN_RSAN};

/// Whether verbose client-side diagnostics are on (`BOT_DEBUG=1`), cached
/// once per process. Used by the scene-build / on-demand paths to dump the
/// loc stream they actually receive, so a live run can be compared against
/// the packed cache data.
pub fn debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("BOT_DEBUG").is_ok_and(|v| v == "1"))
}
