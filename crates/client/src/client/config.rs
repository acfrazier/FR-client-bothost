//! Connection and launch settings for `Client`. Matches the spec's
//! `ClientConfig`: connection params and feature flags only — RSA is baked at
//! compile time (`LOGIN_RSAN` / `LOGIN_RSAE`), never configured here.

pub struct ClientConfig {
    pub host: String,
    pub port: u16,
    pub cache_dir: String,
    pub members: bool,
    pub lowmem: bool,
}
