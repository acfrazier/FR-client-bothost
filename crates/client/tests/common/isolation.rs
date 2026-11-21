use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use client::client::{Client, ClientConfig, ClientRevision};
use client::config::{Cache, IfType, IfTypeMut};

#[path = "../../src/test_support.rs"]
mod test_support;
use test_support::IsolatedEnvironment;

pub struct IsolatedClient {
    client: Client,
    _environment: IsolatedEnvironment,
}

#[allow(dead_code)] // Each integration test uses only part of this shared helper.
impl IsolatedClient {
    pub fn new(revision: ClientRevision, game_server: Option<SocketAddr>) -> Self {
        Self::with_config(
            ClientConfig {
                host: String::new(),
                port: 0,
                cache_dir: String::new(),
                members: true,
                lowmem: false,
            },
            revision,
            game_server,
        )
    }

    pub fn with_config(
        mut config: ClientConfig,
        revision: ClientRevision,
        game_server: Option<SocketAddr>,
    ) -> Self {
        let environment = IsolatedEnvironment::new(game_server);
        config.cache_dir = environment.cache_dir();
        Self::build(config, revision, environment)
    }

    pub fn with_default_config(config: ClientConfig) -> Self {
        Self::with_config(config, ClientRevision::R274, None)
    }

    pub fn with_revision(config: ClientConfig, revision: ClientRevision) -> Self {
        Self::with_config(config, revision, None)
    }

    pub fn with_config_preserving_cache(
        config: ClientConfig,
        revision: ClientRevision,
        game_server: Option<SocketAddr>,
    ) -> Self {
        Self::build(config, revision, IsolatedEnvironment::new(game_server))
    }

    pub fn with_shared(
        mut config: ClientConfig,
        cache: Arc<Cache>,
        ifaces: Arc<Vec<Option<Box<IfType>>>>,
        ifaces_mut: impl Into<Arc<Vec<Option<Arc<IfTypeMut>>>>>,
        revision: ClientRevision,
        game_server: Option<SocketAddr>,
    ) -> Self {
        let environment = IsolatedEnvironment::new(game_server);
        config.cache_dir = environment.cache_dir();
        config.host = environment.game_host().to_owned();
        config.port = environment.game_port();
        let client = Client::from_shared_with_revision(config, cache, ifaces, ifaces_mut, revision);
        Self {
            client,
            _environment: environment,
        }
    }

    fn build(
        mut config: ClientConfig,
        revision: ClientRevision,
        environment: IsolatedEnvironment,
    ) -> Self {
        config.host = environment.game_host().to_owned();
        config.port = environment.game_port();
        let client =
            Client::new_with_revision_and_http_port(config, revision, environment.http_port());
        Self {
            client,
            _environment: environment,
        }
    }

    pub fn into_parts(self) -> (Client, IsolatedEnvironment) {
        (self.client, self._environment)
    }

    pub fn accepted_http_connections(&self) -> usize {
        self._environment.accepted_http_connections()
    }
}

impl Deref for IsolatedClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for IsolatedClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}
