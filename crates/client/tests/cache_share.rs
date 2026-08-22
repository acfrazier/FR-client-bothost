// The immutable config type tables live on an `Arc<Cache>` shared by every
// `Client`, while the mutable interface components (`IfType`, hide/scroll/
// anim/inv slots) stay per-client in `Client.ifaces`. The /tmp cache has no
// packs, so `Client::new` falls back to `Cache::default()`.
use std::sync::Arc;

use client::client::{Client, ClientConfig};
use client::config::{Cache, IfType};

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
    let tables = Arc::new(Cache::default());
    let mut a = Client::new(cfg());
    let mut b = Client::new(cfg());
    a.cache = Arc::clone(&tables);
    b.cache = Arc::clone(&tables);
    assert!(Arc::ptr_eq(&a.cache, &b.cache));
    a.ifaces.resize(2, None);
    a.ifaces[1] = Some(IfType::default());
    a.ifaces[1].as_mut().unwrap().hide = true;
    assert!(b.ifaces.get(1).and_then(|s| s.as_ref()).is_none());
}

/// Host construct: unpack once, then `from_shared` clones the `Arc` and
/// iface template. No `load_cache` / CRC probe; `error_loading` is false.
#[test]
fn from_shared_reuses_one_arc_without_unpack() {
    let tables = Arc::new(Cache::default());
    let template = vec![None, Some(IfType::default())];
    let mut a = Client::from_shared(cfg(), Arc::clone(&tables), template.clone());
    let b = Client::from_shared(cfg(), Arc::clone(&tables), template);
    assert!(Arc::ptr_eq(&a.cache, &b.cache));
    assert!(Arc::ptr_eq(&a.cache, &tables));
    assert!(!a.error_loading);
    assert!(!b.error_loading);
    a.ifaces[1].as_mut().unwrap().hide = true;
    assert!(!b.ifaces[1].as_ref().unwrap().hide);
}
