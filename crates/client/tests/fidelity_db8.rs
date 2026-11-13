//! DB8: Java 289 client.java:12332-12346 always clears icons and redraws brightness.
use client::client::{Client, ClientConfig, ClientRevision};
use client::dash3d::store::ModelStore;
use client::graphics::Pix32;
use std::sync::{Arc, Mutex};

static CACHE_LOCK: Mutex<()> = Mutex::new(());

fn client() -> Client {
    client_with_revision(ClientRevision::R289)
}

fn client_with_revision(revision: ClientRevision) -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: std::env::temp_dir()
                .join(format!("fidelity-dispatch-{}", std::process::id()))
                .display()
                .to_string(),
            members: true,
            lowmem: false,
        },
        revision,
    );
    c.ingame = true;
    c.ptype = -1;
    c
}

#[test]
fn b8_brightness_code_always_sets_redraw_frame() {
    let _guard = CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut c = client();
    c.redraw_frame = false;
    c.apply_clientcode(1, 0);
    assert!(c.redraw_frame);
}

#[test]
fn brightness_always_invalidates_icons_but_only_valid_values_change_pending_brightness() {
    let _guard = CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let mut c = client_with_revision(revision);
        c.pending_brightness = Some(0.8);
        for (value, expected) in [(0, 0.8), (-1, 0.8), (5, 0.8), (3, 0.7), (0, 0.7)] {
            let store = ModelStore::instance();
            store
                .lock()
                .unwrap()
                .put_sprite(Arc::new(Pix32::new(1, 1)), 77);
            assert!(store.lock().unwrap().sprite(77).is_some());
            c.redraw_frame = false;
            c.apply_clientcode(1, value);
            assert!(c.redraw_frame, "{revision:?} brightness {value}");
            assert_eq!(c.pending_brightness, Some(expected));
            assert!(
                store.lock().unwrap().sprite(77).is_none(),
                "{revision:?} brightness {value}"
            );
        }
    }
}
