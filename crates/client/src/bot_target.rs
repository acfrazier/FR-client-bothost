//! Runtime world switch. Cargo's `TARGET` is the rustc triple.
//!
//! `BOT_TARGET=prod` (alias `live`) talks to `w1.rs2b2t.com` with the
//! baked public RSA. Anything else is the **local engine** on loopback.
//! Alpha only supports local; prod exists so a later bin does not need a
//! rebuild to flip worlds.

use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Which world a `Client` logs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotTarget {
    /// Loopback Lost City engine. Keys from `$ENGINE_DIR` / `LOGIN_RSAN`.
    Local,
    /// `w1.rs2b2t.com` with the baked public RSA. Unadvertised in alpha.
    Prod,
}

static OVERRIDE: OnceLock<BotTarget> = OnceLock::new();

/// Parse `BOT_TARGET`. `prod` and `live` are the public world; unset/`local`
/// is the local engine.
pub fn bot_target_from_env(value: Option<&str>) -> BotTarget {
    match value {
        Some("prod") | Some("live") => BotTarget::Prod,
        _ => BotTarget::Local,
    }
}

/// Process override (`--prod`). First call wins.
pub fn set_bot_target(target: BotTarget) {
    let _ = OVERRIDE.set(target);
}

/// Active target: `--prod` override, else `BOT_TARGET`, else local.
pub fn bot_target() -> BotTarget {
    if let Some(t) = OVERRIDE.get() {
        return *t;
    }
    bot_target_from_env(env::var("BOT_TARGET").ok().as_deref())
}

/// TCP host for a target. Port stays 43594.
pub fn world_host_for(target: BotTarget) -> &'static str {
    match target {
        BotTarget::Prod => "w1.rs2b2t.com",
        BotTarget::Local => "127.0.0.1",
    }
}

/// [`world_host_for`] for [`bot_target`].
pub fn world_host() -> String {
    world_host_for(bot_target()).into()
}

/// Lost City engine root (`data/config/private.pem`, `data/pack/client`).
/// `$ENGINE_DIR` if set, else `$HOME/experiments/Server/engine`.
pub fn engine_dir() -> PathBuf {
    if let Ok(p) = env::var("ENGINE_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    match env::var("HOME") {
        Ok(home) => PathBuf::from(home).join("experiments/Server/engine"),
        Err(_) => PathBuf::from("experiments/Server/engine"),
    }
}

/// Client pack cache: `$ENGINE_DIR/data/pack/client`.
pub fn cache_dir() -> PathBuf {
    engine_dir().join("data/pack/client")
}

/// Config jag: `$ENGINE_DIR/data/pack/config`.
pub fn config_jag() -> PathBuf {
    engine_dir().join("data/pack/config")
}

/// Engine RSA private key: `$ENGINE_DIR/data/config/private.pem`.
pub fn private_pem() -> PathBuf {
    engine_dir().join("data/config/private.pem")
}

/// Server content tree (maps, loc scripts): sibling of `engine/` named
/// `content/`.
pub fn content_dir() -> PathBuf {
    match engine_dir().parent() {
        Some(root) => root.join("content"),
        None => PathBuf::from("content"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn prod_and_live_alias_are_the_public_world() {
        assert_eq!(bot_target_from_env(Some("prod")), BotTarget::Prod);
        assert_eq!(bot_target_from_env(Some("live")), BotTarget::Prod);
        assert_eq!(bot_target_from_env(Some("local")), BotTarget::Local);
        assert_eq!(bot_target_from_env(None), BotTarget::Local);
        assert_eq!(world_host_for(BotTarget::Prod), "w1.rs2b2t.com");
        assert_eq!(world_host_for(BotTarget::Local), "127.0.0.1");
    }

    #[test]
    fn content_sits_next_to_engine() {
        let maps = Path::new("/tmp/Server/engine");
        assert_eq!(
            maps.parent().unwrap().join("content/maps"),
            Path::new("/tmp/Server/content/maps")
        );
    }
}
