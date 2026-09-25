use std::path::{Path, PathBuf};
use std::str::FromStr;

use num_bigint::BigUint;

use crate::client::ClientConfig;
use crate::io::ClientRevision;
use crate::BotTarget;

/// Owned inputs used to freeze one client's connection and resource identity.
#[derive(Clone, Debug)]
pub struct ClientSessionConfig {
    pub revision: ClientRevision,
    pub target: BotTarget,
    pub game_host: String,
    pub game_port: u16,
    pub asset_host: String,
    pub asset_port: u16,
    pub cache_dir: PathBuf,
    pub unpack_dir: PathBuf,
    pub rsa_modulus: String,
    pub rsa_exponent: String,
    pub expected_crc: Option<[i32; 9]>,
    pub content_id: String,
    pub file_store_dir: Option<PathBuf>,
    pub ondemand_persist_dir: Option<PathBuf>,
}

/// Immutable connection and resource identity shared by every slot in a session.
#[derive(Debug, PartialEq, Eq)]
pub struct ClientSessionProfile {
    revision: ClientRevision,
    target: BotTarget,
    game_host: String,
    game_port: u16,
    asset_host: String,
    asset_port: u16,
    cache_dir: PathBuf,
    unpack_dir: PathBuf,
    rsa_modulus: String,
    rsa_exponent: String,
    rsa_modulus_value: BigUint,
    rsa_exponent_value: BigUint,
    expected_crc: Option<[i32; 9]>,
    content_id: String,
    file_store_dir: Option<PathBuf>,
    ondemand_persist_dir: Option<PathBuf>,
}

impl ClientSessionProfile {
    pub fn new(config: ClientSessionConfig) -> Result<Self, String> {
        if config.game_host.trim().is_empty() {
            return Err("game_host must not be empty".into());
        }
        if config.game_port == 0 {
            return Err("game_port must not be zero".into());
        }
        if config.asset_host.trim().is_empty() {
            return Err("asset_host must not be empty".into());
        }
        if config.asset_port == 0 {
            return Err("asset_port must not be zero".into());
        }
        if config.cache_dir.as_os_str().is_empty() {
            return Err("cache_dir must not be empty".into());
        }
        if config.cache_dir.to_str().is_none() {
            return Err("cache_dir must be valid UTF-8 for ClientConfig".into());
        }
        if config
            .file_store_dir
            .as_ref()
            .is_some_and(|path| path.to_str().is_none())
        {
            return Err("file_store_dir must be valid UTF-8".into());
        }
        if config
            .ondemand_persist_dir
            .as_ref()
            .is_some_and(|path| path.to_str().is_none())
        {
            return Err("ondemand_persist_dir must be valid UTF-8".into());
        }
        if config.unpack_dir.as_os_str().is_empty() {
            return Err("unpack_dir must not be empty".into());
        }
        if config.content_id.is_empty() {
            return Err("content_id must not be empty".into());
        }
        let rsa_modulus_value = BigUint::from_str(&config.rsa_modulus)
            .map_err(|_| "RSA modulus must be a positive decimal integer".to_string())?;
        if rsa_modulus_value == BigUint::from(0u8) {
            return Err("RSA modulus must be a positive decimal integer".into());
        }
        let rsa_exponent_value = BigUint::from_str(&config.rsa_exponent)
            .map_err(|_| "RSA exponent must be a positive decimal integer".to_string())?;
        if rsa_exponent_value == BigUint::from(0u8) {
            return Err("RSA exponent must be a positive decimal integer".into());
        }

        Ok(Self {
            revision: config.revision,
            target: config.target,
            game_host: config.game_host,
            game_port: config.game_port,
            asset_host: config.asset_host,
            asset_port: config.asset_port,
            cache_dir: config.cache_dir,
            unpack_dir: config.unpack_dir,
            rsa_modulus: config.rsa_modulus,
            rsa_exponent: config.rsa_exponent,
            rsa_modulus_value,
            rsa_exponent_value,
            expected_crc: config.expected_crc,
            content_id: config.content_id,
            file_store_dir: config.file_store_dir,
            ondemand_persist_dir: config.ondemand_persist_dir,
        })
    }

    pub fn revision(&self) -> ClientRevision {
        self.revision
    }

    pub fn target(&self) -> BotTarget {
        self.target
    }

    pub fn game_host(&self) -> &str {
        &self.game_host
    }

    pub fn game_port(&self) -> u16 {
        self.game_port
    }

    pub fn asset_host(&self) -> &str {
        &self.asset_host
    }

    pub fn asset_port(&self) -> u16 {
        self.asset_port
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn unpack_dir(&self) -> &Path {
        &self.unpack_dir
    }

    pub fn rsa_modulus(&self) -> &str {
        &self.rsa_modulus
    }

    pub fn rsa_exponent(&self) -> &str {
        &self.rsa_exponent
    }

    pub fn expected_crc(&self) -> Option<[i32; 9]> {
        self.expected_crc
    }

    pub fn content_id(&self) -> &str {
        &self.content_id
    }

    pub fn file_store_dir(&self) -> Option<&Path> {
        self.file_store_dir.as_deref()
    }

    pub fn ondemand_persist_dir(&self) -> Option<&Path> {
        self.ondemand_persist_dir.as_deref()
    }

    /// Change only the login endpoint and key; shared assets stay bound to the
    /// template's original update server and cache identity.
    pub fn for_public_world(&self, host: &str, port: u16, modulus: &str) -> Result<Self, String> {
        if self.target != BotTarget::Prod {
            return Err("world switching requires a public session".into());
        }
        Self::new(ClientSessionConfig {
            revision: self.revision,
            target: self.target,
            game_host: host.into(),
            game_port: port,
            asset_host: self.asset_host.clone(),
            asset_port: self.asset_port,
            cache_dir: self.cache_dir.clone(),
            unpack_dir: self.unpack_dir.clone(),
            rsa_modulus: modulus.into(),
            rsa_exponent: self.rsa_exponent.clone(),
            expected_crc: self.expected_crc,
            content_id: self.content_id.clone(),
            file_store_dir: self.file_store_dir.clone(),
            ondemand_persist_dir: self.ondemand_persist_dir.clone(),
        })
    }

    pub fn client_config(&self, members: bool, lowmem: bool) -> ClientConfig {
        ClientConfig {
            host: self.game_host.clone(),
            port: self.game_port,
            cache_dir: self
                .cache_dir
                .to_str()
                .expect("validated UTF-8 cache_dir")
                .to_string(),
            members,
            lowmem,
        }
    }

    pub(crate) fn rsa_biguints(&self) -> (&BigUint, &BigUint) {
        (&self.rsa_modulus_value, &self.rsa_exponent_value)
    }
}
