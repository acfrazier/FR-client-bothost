// Immutable type tables may be shared, while mutable interface components
// remain per-client. Each constructor starts from an isolated empty cache.
#[path = "common/isolation.rs"]
mod isolation;

use isolation::IsolatedClient;
use std::sync::Arc;

use client::config::{Cache, IfType, ObjType};

#[test]
fn two_clients_share_type_tables_and_not_ifaces() {
    let mut tables = Cache::default();
    tables.objs.resize(1, ObjType::default());
    tables.objs[0].name = "Coins".into();
    let tables = Arc::new(tables);
    let mut a = IsolatedClient::new(client::client::ClientRevision::R274, None);
    let mut b = IsolatedClient::new(client::client::ClientRevision::R274, None);
    a.cache = Arc::clone(&tables);
    b.cache = Arc::clone(&tables);
    assert!(Arc::ptr_eq(&a.cache, &b.cache));
    assert_eq!(a.cache.obj(0).name, "Coins");
    assert_eq!(b.cache.obj(0).name, "Coins");
    // ifaces stay per-client: b's slot 1 must not reflect a's mutation
    // The empty-cache fixture has no components; b must still be unchanged.
    let b_before = b.if_(1).map(|s| (s.id, s.hide));
    a.set_iface(1, IfType::default());
    a.iface_mut(1).unwrap().hide = true;
    let b_after = b.if_(1).map(|s| (s.id, s.hide));
    assert_eq!(b_after, b_before);
}
