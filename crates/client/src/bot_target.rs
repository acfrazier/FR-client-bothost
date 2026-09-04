//! Runtime world switch. Cargo's `TARGET` is the rustc triple.
//!
//! `BOT_TARGET=prod` (alias `live`) talks to `w1.rs2b2t.com` with the
//! baked public RSA. Anything else is the **local engine** on loopback.
//! Prod is HTTPS `/crc`+jags and WSS `ClientStream`; local stays TCP.

use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Which world a `Client` logs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotTarget {
    /// Loopback Lost City engine. Keys from `$ENGINE_DIR` / `LOGIN_RSAN`.
    Local,
    /// `w1.rs2b2t.com` with the baked public RSA.
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

/// TCP host for a target.
pub fn world_host_for(target: BotTarget) -> &'static str {
    match target {
        BotTarget::Prod => "w1.rs2b2t.com",
        BotTarget::Local => "127.0.0.1",
    }
}

/// Game port. Local is Java TCP `:43594`. Prod is WSS on `:443`.
pub fn game_port_for(target: BotTarget) -> u16 {
    match target {
        BotTarget::Prod => 443,
        BotTarget::Local => 43594,
    }
}

/// Jag/crc fetch port. Local HTTP `:80`; Prod HTTPS `:443`.
pub fn jag_fetch_port_for(target: BotTarget) -> u16 {
    match target {
        BotTarget::Prod => 443,
        BotTarget::Local => 80,
    }
}

/// Prod talks WSS + HTTPS; local stays TCP + HTTP.
pub fn uses_secure_transport(target: BotTarget) -> bool {
    target == BotTarget::Prod
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

/// Jag pack + versioned snapshots (`models.bin` etc.). Prod downloads land here.
pub fn unpack_dir() -> PathBuf {
    match env::var("HOME") {
        Ok(home) if !home.is_empty() => PathBuf::from(home).join(".274bot/unpack"),
        _ => PathBuf::from(".274bot/unpack"),
    }
}

/// Pack cache for a target. Local is the engine jag dir; Prod is
/// [`unpack_dir`] so HTTPS `/crc`+jags do not overwrite the local engine pack.
/// Versioned snapshots stay in `{unpack_dir}/{sha256(versionlist)[:8]}/`.
pub fn cache_dir_for(target: BotTarget) -> PathBuf {
    match target {
        BotTarget::Local => engine_dir().join("data/pack/client"),
        BotTarget::Prod => unpack_dir(),
    }
}

/// [`cache_dir_for`] for [`bot_target`].
pub fn cache_dir() -> PathBuf {
    cache_dir_for(bot_target())
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
        assert_eq!(game_port_for(BotTarget::Prod), 443);
        assert_eq!(game_port_for(BotTarget::Local), 43594);
        assert_eq!(jag_fetch_port_for(BotTarget::Prod), 443);
        assert_eq!(jag_fetch_port_for(BotTarget::Local), 80);
        assert!(uses_secure_transport(BotTarget::Prod));
        assert!(!uses_secure_transport(BotTarget::Local));
    }

    #[test]
    fn prod_cache_dir_is_home_unpack_not_engine_pack() {
        assert_eq!(
            cache_dir_for(BotTarget::Local),
            engine_dir().join("data/pack/client")
        );
        assert_eq!(cache_dir_for(BotTarget::Prod), unpack_dir());
        assert_ne!(
            cache_dir_for(BotTarget::Prod),
            cache_dir_for(BotTarget::Local),
            "prod jag downloads must not land in the local engine pack"
        );
        let unpack = unpack_dir();
        assert_eq!(
            unpack.file_name().map(|s| s.to_string_lossy().into_owned()),
            Some("unpack".into())
        );
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
