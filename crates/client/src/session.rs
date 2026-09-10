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
