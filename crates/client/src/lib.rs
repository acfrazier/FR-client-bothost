//! 274 client library for the 274bot host (`FR-client-bothost`
//! `r274-bh-modular`). wgpu GPU 3D by default; `BOT_CPU=1` is CpuPix3D.
//! No bot action API — packet timing and `doAction` stay Java-shaped.

pub mod bot_target;
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
pub use bot_target::{
    bot_target, cache_dir, cache_dir_for, content_dir, engine_dir, game_port_for,
    jag_fetch_port_for, set_bot_target, unpack_dir, uses_secure_transport, world_host,
    world_host_for, BotTarget,
};
pub use login_rsa::{
    active_pair, JAVA_LOGIN_RSAE, JAVA_LOGIN_RSAN, PROD_LOGIN_RSAE, PROD_LOGIN_RSAN,
};

/// Whether verbose client-side diagnostics are on (`BOT_DEBUG=1`), cached
/// once per process. Used by the scene-build / on-demand paths to dump the
/// loc stream they actually receive, so a live run can be compared against
/// the packed cache data. Render dumps are [`render_debug_enabled`].
pub fn debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("BOT_DEBUG").is_ok_and(|v| v == "1"))
}

/// GPU/CPU render dumps (`[gpu-emit]`, `[winding]`, atlas, …). Separate
/// from [`debug_enabled`] so nav live traces are not drowned. `BOT_RENDER_DEBUG=1`.
pub fn render_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("BOT_RENDER_DEBUG").is_ok_and(|v| v == "1"))
}
