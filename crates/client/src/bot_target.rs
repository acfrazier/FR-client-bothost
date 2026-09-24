//! Runtime world switch. Cargo's `TARGET` is the rustc triple.
//!
//! An unbound `BOT_TARGET=prod` client defaults to `w1.rs2b2t.com` with
//! the baked public RSA. The bot host binds its configured world and runtime
//! login key per slot. Prod uses HTTPS `/crc`+jags and WSS `ClientStream`;
//! local stays TCP.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Operator home for path defaults (`~/.274bot`, engine under `$HOME/...`).
///
/// Same `Result` shape as `env::var("HOME")`. On non-Windows this is
/// verbatim `HOME`. On Windows, an explicitly set `HOME` (including empty)
/// always wins; `USERPROFILE` is used only when `HOME` is absent or not
/// valid Unicode. Callers keep their own empty/fallback handling.
pub fn operator_home() -> Result<String, env::VarError> {
    operator_home_from(env::var("HOME"), || env::var("USERPROFILE"))
}

/// Pure selector for tests: inject `HOME` / `USERPROFILE` results without
/// mutating the process environment.
fn operator_home_from(
    home: Result<String, env::VarError>,
    userprofile: impl FnOnce() -> Result<String, env::VarError>,
) -> Result<String, env::VarError> {
    #[cfg(windows)]
    {
        match home {
            Ok(h) => Ok(h),
            Err(_) => userprofile(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = userprofile;
        home
    }
}

/// Which world a `Client` logs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotTarget {
    /// Loopback Lost City engine. Keys from `$ENGINE_DIR` / `LOGIN_RSAN`.
    Local,
    /// Public transport; an unbound client defaults to w1 with baked RSA.
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
    match operator_home() {
        Ok(home) => PathBuf::from(home).join("experiments/Server/engine"),
        Err(_) => PathBuf::from("experiments/Server/engine"),
    }
}

/// Jag pack + versioned snapshots (`models.bin` etc.). Prod downloads land here.
/// `$CLIENT_UNPACK_DIR` overrides the legacy `$HOME/.274bot/unpack` default.
pub fn unpack_dir() -> PathBuf {
    unpack_dir_from_env(
        env::var("CLIENT_UNPACK_DIR").ok().as_deref(),
        operator_home().ok().as_deref(),
    )
}

fn unpack_dir_from_env(client_unpack_dir: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(path) = client_unpack_dir.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    match home.filter(|path| !path.is_empty()) {
        Some(home) => PathBuf::from(home).join(".274bot/unpack"),
        None => PathBuf::from(".274bot/unpack"),
    }
}

/// Pack cache for a target. Local is the engine jag dir; Prod is
/// [`unpack_dir`] so HTTPS `/crc`+jags do not overwrite the local engine pack.
/// Versioned snapshots stay in `{unpack_dir}/{sha256(versionlist)[:8]}/`.
pub fn cache_dir_for(target: BotTarget) -> PathBuf {
    cache_dir_for_with_unpack(target, &unpack_dir())
}

fn cache_dir_for_with_unpack(target: BotTarget, unpack_dir: &Path) -> PathBuf {
    match target {
        BotTarget::Local => engine_dir().join("data/pack/client"),
        BotTarget::Prod => unpack_dir.to_path_buf(),
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
    fn unpack_dir_override_is_explicit_and_target_specific() {
        let override_dir = Path::new("/tmp/client-289-unpack");
        assert_eq!(
            unpack_dir_from_env(Some(override_dir.to_str().unwrap()), Some("/home/test")),
            override_dir
        );
        assert_eq!(
            cache_dir_for_with_unpack(BotTarget::Prod, override_dir),
            override_dir
        );
        assert_eq!(
            cache_dir_for_with_unpack(BotTarget::Local, override_dir),
            engine_dir().join("data/pack/client")
        );
    }

    #[test]
    fn unpack_dir_override_empty_or_absent_preserves_home_default() {
        let home = Some("/home/test");
        let default = Path::new("/home/test/.274bot/unpack");
        assert_eq!(unpack_dir_from_env(None, home), default);
        assert_eq!(unpack_dir_from_env(Some(""), home), default);
    }

    #[test]
    fn content_sits_next_to_engine() {
        let maps = Path::new("/tmp/Server/engine");
        assert_eq!(
            maps.parent().unwrap().join("content/maps"),
            Path::new("/tmp/Server/content/maps")
        );
    }

    #[test]
    fn operator_home_prefers_explicit_home_including_empty() {
        use std::env::VarError;
        assert_eq!(
            operator_home_from(Ok("/explicit".into()), || Ok("/profile".into())).unwrap(),
            "/explicit"
        );
        assert_eq!(
            operator_home_from(Ok(String::new()), || Ok("/profile".into())).unwrap(),
            ""
        );
        let missing = Err(VarError::NotPresent);
        #[cfg(windows)]
        {
            assert_eq!(
                operator_home_from(missing.clone(), || Ok(r"C:\Users\op".into())).unwrap(),
                r"C:\Users\op"
            );
            assert!(operator_home_from(missing.clone(), || missing.clone()).is_err());
            assert_eq!(
                operator_home_from(Err(VarError::NotUnicode("x".into())), || Ok("/p".into()))
                    .unwrap(),
                "/p"
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(
                operator_home_from(Ok("/unix".into()), || Ok("/ignored".into())).unwrap(),
                "/unix"
            );
            assert!(operator_home_from(missing.clone(), || Ok("/ignored".into())).is_err());
            assert!(operator_home_from(missing.clone(), || missing).is_err());
        }
    }
    #[test]
    fn operator_home_never_queries_profile_for_explicit_home() {
        assert_eq!(
            super::operator_home_from(Ok("/explicit".into()), || panic!(
                "unexpected USERPROFILE read"
            ))
            .unwrap(),
            "/explicit"
        );
        #[cfg(not(windows))]
        assert!(
            super::operator_home_from(Err(std::env::VarError::NotPresent), || panic!(
                "Unix queried USERPROFILE"
            ))
            .is_err()
        );
    }
}
