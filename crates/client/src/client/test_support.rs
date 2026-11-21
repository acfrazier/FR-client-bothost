use crate::client::{Client, ClientConfig, ClientRevision};
use crate::test_support::IsolatedEnvironment;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};

pub(crate) fn isolated_client(
    config: ClientConfig,
    revision: ClientRevision,
) -> (Client, IsolatedEnvironment) {
    isolated_client_with_game_server(config, revision, None)
}

pub(crate) fn isolated_client_with_game_server(
    mut config: ClientConfig,
    revision: ClientRevision,
    game_server: Option<SocketAddr>,
) -> (Client, IsolatedEnvironment) {
    let environment = IsolatedEnvironment::new(game_server);
    config.host = environment.game_host().to_owned();
    config.port = environment.game_port();
    config.cache_dir = environment.cache_dir();
    let client = Client::new_with_revision_and_http_port(config, revision, environment.http_port());
    (client, environment)
}

pub(crate) struct IsolatedClient {
    client: Client,
    _environment: IsolatedEnvironment,
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

pub(crate) fn isolated_client_handle(
    config: ClientConfig,
    revision: ClientRevision,
) -> IsolatedClient {
    let (client, environment) = isolated_client(config, revision);
    IsolatedClient {
        client,
        _environment: environment,
    }
}
