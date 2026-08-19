// IF_SETICON / IF_SHOWICON handlers and the side-tab state they drive
// (side_icon / active_icon / side_modal_id / chat_modal_id). The /tmp cache
// has no packs, so `Client::new` falls back to `Cache::default()` and never
// touches the network (the /crc fetch on 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig};
use client::io::Packet;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn icon_state_defaults() {
    let c = client();
    assert_eq!(c.active_icon, 3);
    assert!(c.side_icon.iter().all(|&id| id == -1));
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
}

#[test]
fn if_seticon_writes_side_icon_slot() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 50, 3]); // com 50, tab 3
    c.apply_if_seticon(&mut p);
    assert_eq!(c.side_icon[3], 50);
    assert!(c.redraw_side && c.redraw_icons);
}

#[test]
fn if_seticon_65535_clears_slot() {
    let mut c = client();
    let mut p = Packet::new(vec![0xff, 0xff, 0]); // com 65535 -> -1, tab 0
    c.apply_if_seticon(&mut p);
    assert_eq!(c.side_icon[0], -1);
}

#[test]
fn if_seticon_out_of_range_icon_is_ignored() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 50, 14]); // tab 14 is outside 0..14
    c.apply_if_seticon(&mut p);
    assert_eq!(c.side_icon, [-1; 14]);
}

#[test]
fn if_showicon_sets_active_icon() {
    let mut c = client();
    c.apply_if_showicon(7);
    assert_eq!(c.active_icon, 7);
    assert!(c.redraw_side && c.redraw_icons);
}

#[test]
fn sideicons_load_from_media() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.game_draw();
    assert!(c.sideicons.iter().any(|s| s.is_some()));
    assert!(c.redstone1.is_some() && c.redstone2.is_some() && c.redstone3.is_some());
    assert!(c.redstone1h.is_some() && c.redstone2hv.is_some());
}

#[test]
fn click_combat_tab_sets_active_icon_0() {
    let mut c = client();
    c.ingame = true;
    c.side_icon[0] = 1; // tab present
    c.shell.apply_mouse_down(1, 550, 180);
    c.shell.latch_click();
    c.handle_tab_clicks();
    assert_eq!(c.active_icon, 0);
}
