//! Connection and launch settings for `Client`. Matches the spec's
//! `ClientConfig`: connection params and feature flags only. RSA is chosen
//! at login from [`crate::bot_target::BotTarget`] (baked prod vs local pem).

pub struct ClientConfig {
    pub host: String,
    pub port: u16,
    pub cache_dir: String,
    pub members: bool,
    pub lowmem: bool,
}
