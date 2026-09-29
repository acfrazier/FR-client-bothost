//! `ClientConfig`: connection params and feature flags only.
//! Bound sessions carry transport and RSA in `ClientSessionProfile`.

pub struct ClientConfig {
    pub host: String,
    pub port: u16,
    pub cache_dir: String,
    pub members: bool,
    pub lowmem: bool,
}
