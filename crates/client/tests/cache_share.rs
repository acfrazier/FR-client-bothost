// The immutable config type tables live on an `Arc<Cache>` shared by every
// `Client`, while the mutable interface components (`IfType`, hide/scroll/
// anim/inv slots) stay per-client in `Client.ifaces`. The /tmp cache has no
// packs, so `Client::new` falls back to `Cache::default()`.
use std::sync::Arc;

use client::client::{Client, ClientConfig};
use client::config::{Cache, IfType, ObjType};

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

#[test]
fn two_clients_share_type_tables_and_not_ifaces() {
    let mut tables = Cache::default();
    tables.objs.resize(1, ObjType::default());
    tables.objs[0].name = "Coins".into();
    let tables = Arc::new(tables);
    let mut a = Client::new(cfg());
    let mut b = Client::new(cfg());
    a.cache = Arc::clone(&tables);
    b.cache = Arc::clone(&tables);
    assert!(Arc::ptr_eq(&a.cache, &b.cache));
    assert_eq!(a.cache.obj(0).name, "Coins");
    assert_eq!(b.cache.obj(0).name, "Coins");
    // ifaces stay per-client: b's slot 1 must not reflect a's mutation
    // (the /tmp cache may or may not have unpacked real components).
    let b_before = b.if_(1).map(|s| (s.id, s.hide));
    a.set_iface(1, IfType::default());
    a.iface_mut(1).unwrap().hide = true;
    let b_after = b.if_(1).map(|s| (s.id, s.hide));
    assert_eq!(b_after, b_before);
}
