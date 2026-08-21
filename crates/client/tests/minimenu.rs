// Minimenu chrome: `open_menu` clamps the menu into the panel holding the
// click (0 viewport, 1 side, 2 chat) and sizes it to the widest option.
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and never touches the network (the /crc fetch on
// 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig};

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
fn open_menu_in_viewport_sets_area_0_and_geometry() {
    let mut c = client();
    c.menu_num_entries = 3;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Walk here".into();
    c.menu_option[2] = "Examine @cya@Tree".into();
    c.shell.mouse_click_x = 100;
    c.shell.mouse_click_y = 100;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 0);
    assert!(c.menu_width >= 8);
    assert_eq!(c.menu_height, 3 * 15 + 22);
}

#[test]
fn open_menu_in_side_sets_area_1() {
    let mut c = client();
    c.menu_num_entries = 2;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Wear".into();
    c.shell.mouse_click_x = 600;
    c.shell.mouse_click_y = 300;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 1);
}

#[test]
fn open_menu_in_chat_sets_area_2() {
    let mut c = client();
    c.menu_num_entries = 2;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Report abuse".into();
    c.shell.mouse_click_x = 100;
    c.shell.mouse_click_y = 400;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 2);
}

#[test]
fn open_menu_clamps_viewport_menu_inside_512x334() {
    let mut c = client();
    c.menu_num_entries = 3;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Walk here".into();
    c.menu_option[2] = "Examine @cya@Tree".into();
    c.shell.mouse_click_x = 514;
    c.shell.mouse_click_y = 330;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 0);
    assert_eq!(c.menu_x + c.menu_width, 512);
    // The y-clamp fits the 15*3+21 height (TS 8473-8478); the stored
    // `menu_height` is 15*3+22 (TS 8481), one taller, kept verbatim.
    assert_eq!(c.menu_y, 334 - (3 * 15 + 21));
    assert_eq!(c.menu_height, 3 * 15 + 22);
}
