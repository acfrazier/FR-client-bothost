//! Task 1: `clientButton` CC_LOGOUT arm (Java `Client.java` 8745-8747).
//! Clicking a control with client code 205 arms `logoutTimer` (250 frames,
//! ~5 s at 20 ms); unported client codes return `true` so the existing
//! unconditional `IF_BUTTON` send is preserved (operator-accepted deferral,
//! 2026-08-20 — the full `clientButton` port is slice 3/5). The
//! `handle_side_if_clicks` hook fires only in the BUTTON_OK arm, matching
//! Java `execute` (`var5 == 231`, `Client.java` 4562).
use client::client::{Client, ClientConfig, APPLET_H, APPLET_W};
use client::config::if_type::{ButtonType, ComponentType, IfType};
use client::graphics::PixMap;

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
fn cc_logout_arms_logout_timer() {
    let mut c = client();
    let com = IfType {
        client_code: 205,
        ..IfType::default()
    };
    assert!(c.client_button(&com));
    assert_eq!(c.logout_timer, 250);
    let other = IfType {
        client_code: 3,
        ..IfType::default()
    };
    assert!(c.client_button(&other));
    assert_eq!(c.logout_timer, 250); // unchanged for unported codes
}

#[test]
fn cc_logout_hook_ignores_non_ok_buttons() {
    let mut c = client();
    // Java execute calls clientButton only from the var5 == 231 (BUTTON_OK)
    // arm; a BUTTON_TOGGLE with client code 205 must not arm the timer.
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 20);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_TOGGLE,
        client_code: 205,
        width: 190,
        height: 20,
        ..IfType::default()
    };
    bind_side(&mut c, 1, vec![root, toggle]);
    click_side(&mut c, 560, 210);
    assert_eq!(c.logout_timer, 0);
}

/// Bind `components` into the cache and `root` onto the active side tab (3).
fn bind_side(c: &mut Client, root: i32, components: Vec<IfType>) {
    c.ingame = true;
    c.side_icon[3] = root;
    c.active_icon = 3;
    let max = components.iter().map(|com| com.id).max().unwrap_or(0) as usize;
    c.cache.ifaces.resize(max + 1, None);
    for com in components {
        let id = com.id as usize;
        c.cache.ifaces[id] = Some(com);
    }
}

/// Latch a left click at applet coords and run the side click handler.
fn click_side(c: &mut Client, x: i32, y: i32) {
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.handle_side_if_clicks();
}

/// A TYPE_LAYER with one child list (child offsets and layer size).
fn side_layer(
    id: i32,
    children: Vec<i32>,
    child_x: Vec<i32>,
    child_y: Vec<i32>,
    width: i32,
    height: i32,
) -> IfType {
    IfType {
        id,
        r#type: ComponentType::TYPE_LAYER,
        width,
        height,
        children: Some(children),
        child_x: Some(child_x),
        child_y: Some(child_y),
        ..IfType::default()
    }
}

// --- Task 4: `logout()` restores the title frame ---
// Java `logout` (`Client.java` 6357-6379) drops `ingame`/`loginscreen` and
// clears the caches; the title rebuild is `prepareGame`+`prepareTitle`
// parity — `unload_title` + `image_title2 = None` so the next
// `prepare_title` reallocates the 9 regions, plus `redraw_frame = true` and
// a one-shot `draw_area` cls so no game-frame pixel can survive.

#[test]
fn logout_restores_title_frame() {
    let mut c = client();
    c.ingame = true;
    c.loginscreen = 2;
    c.redraw_frame = false;
    c.draw_area = PixMap::new(APPLET_W, APPLET_H);
    c.draw_area.fill(0x00ff00);
    c.logout();
    assert!(!c.ingame, "logout must leave the game state");
    assert_eq!(c.loginscreen, 0, "logout returns to the welcome screen");
    assert!(c.redraw_frame, "logout must force a full title redraw");
    assert!(
        c.image_title2.is_none(),
        "logout must drop image_title2 so prepare_title reallocates"
    );
    assert!(
        c.draw_area.pixels.iter().all(|&p| p == 0),
        "logout must clear draw_area so no game-frame pixel survives"
    );
}

#[test]
fn logout_then_title_draw_reallocates_regions() {
    let mut c = client();
    c.ingame = true;
    c.loginscreen = 2;
    c.redraw_frame = false;
    c.draw_area = PixMap::new(APPLET_W, APPLET_H);
    c.draw_area.fill(0x00ff00);
    c.logout();
    assert!(c.image_title2.is_none());
    c.title_screen_draw();
    assert!(
        c.image_title2.is_some(),
        "the next title draw must reallocate the title regions"
    );
    assert!(c.image_title0.is_some() && c.image_title1.is_some());
}
