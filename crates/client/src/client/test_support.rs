use crate::client::{Client, ClientConfig, ClientRevision};
use crate::test_support::IsolatedEnvironment;

pub(crate) fn isolated_client(
    mut config: ClientConfig,
    revision: ClientRevision,
) -> (Client, IsolatedEnvironment) {
    let environment = IsolatedEnvironment::new(None);
    config.host = environment.game_host().to_owned();
    config.port = environment.game_port();
    config.cache_dir = environment.cache_dir();
    let client = Client::new_with_revision_and_http_port(config, revision, environment.http_port());
    (client, environment)
}
