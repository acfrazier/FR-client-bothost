//! Explicit game and asset transport selected by the embedding host.

use std::env;
use std::path::PathBuf;

/// The paired game/asset transport for one bound client session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// Plain TCP game socket and HTTP asset requests.
    Tcp,
    /// TLS WebSocket game socket and HTTPS asset requests.
    Wss,
}

impl Transport {
    pub const fn uses_tls(self) -> bool {
        matches!(self, Self::Wss)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Wss => "wss",
        }
    }
}

/// Operator home for path defaults (`~/.274bot`, engine under `$HOME/...`).
pub fn operator_home() -> Result<String, env::VarError> {
    operator_home_from(env::var("HOME"), || env::var("USERPROFILE"))
}

fn operator_home_from(
    home: Result<String, env::VarError>,
    userprofile: impl FnOnce() -> Result<String, env::VarError>,
) -> Result<String, env::VarError> {
    #[cfg(windows)]
    {
        match home {
            Ok(home) => Ok(home),
            Err(_) => userprofile(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = userprofile;
        home
    }
}

/// Lost City engine root used by standalone client tooling and tests.
pub fn engine_dir() -> PathBuf {
    if let Ok(path) = env::var("ENGINE_DIR") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    match operator_home() {
        Ok(home) => PathBuf::from(home).join("experiments/Server/engine"),
        Err(_) => PathBuf::from("experiments/Server/engine"),
    }
}

/// Standalone-client unpack directory. Bound host sessions supply their own.
pub fn unpack_dir() -> PathBuf {
    if let Ok(path) = env::var("CLIENT_UNPACK_DIR") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    match operator_home().ok().filter(|home| !home.is_empty()) {
        Some(home) => PathBuf::from(home).join(".274bot/unpack"),
        None => PathBuf::from(".274bot/unpack"),
    }
}

/// Standalone local-client cache directory. Launch profiles own runtime caches.
pub fn cache_dir() -> PathBuf {
    engine_dir().join("data/pack/client")
}

/// Config jag used by standalone tooling.
pub fn config_jag() -> PathBuf {
    engine_dir().join("data/pack/config")
}

/// Engine RSA private key used only by the standalone local client.
pub fn private_pem() -> PathBuf {
    engine_dir().join("data/config/private.pem")
}

/// Server content tree used by standalone tooling.
pub fn content_dir() -> PathBuf {
    match engine_dir().parent() {
        Some(root) => root.join("content"),
        None => PathBuf::from("content"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transports_name_the_actual_wire() {
        assert_eq!(Transport::Tcp.as_str(), "tcp");
        assert!(!Transport::Tcp.uses_tls());
        assert_eq!(Transport::Wss.as_str(), "wss");
        assert!(Transport::Wss.uses_tls());
    }
}
