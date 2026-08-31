//! Task 1: `clientButton` CC_LOGOUT arm (Java `Client.java` 8745-8747).
//! Clicking a control with client code 205 arms `logoutTimer` (250 frames,
//! ~5 s at 20 ms); the social (200-202/500-502) and player-design
//! (300-327) codes return `false` so the `IF_BUTTON` send is vetoed.
//! Clicks reach `clientButton` through the `doAction` IF_BUTTON arm
//! (TS 9144-9154), and the non-OK button arms never call it.
use client::client::{Client, ClientConfig, APPLET_H, APPLET_W};
use client::config::if_type::{ButtonType, ComponentType, IfType, IfTypeMut};
use client::graphics::PixMap;
use client::render::Renderer;

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
    let _r = Renderer::new(false);
    let mut c = client();
    c.set_iface(
        9,
        IfType {
            client_code: 205,
            ..IfType::default()
        },
    );
    assert!(c.client_button(9));
    assert_eq!(c.logout_timer, 250);
    c.set_iface(
        9,
        IfType {
            client_code: 3,
            ..IfType::default()
        },
    );
    assert!(
        !c.client_button(9),
        "Java clientButton returns false for non-205 codes"
    );
    assert_eq!(c.logout_timer, 250); // unchanged for unported codes
}

#[test]
fn cc_add_friend_opens_social_input_without_if_button() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.friend_server_status = 2;
    c.set_iface(
        9,
        IfType {
            client_code: 201, // CC_ADD_FRIEND
            ..IfType::default()
        },
    );
    assert!(!c.client_button(9), "social codes do not send IF_BUTTON");
    assert!(c.social_input_open);
    assert_eq!(c.social_input_type, 1);
    assert_eq!(c.social_input_header, "Enter name of friend to add to list");
}

#[test]
fn cc_add_ignore_opens_social_input_without_friend_server() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.set_iface(
        9,
        IfType {
            client_code: 501, // CC_ADD_IGNORE
            ..IfType::default()
        },
    );
    assert!(!c.client_button(9));
    assert!(c.social_input_open);
    assert_eq!(c.social_input_type, 4);
}

#[test]
fn cc_logout_hook_ignores_non_ok_buttons() {
    let _r = Renderer::new(false);
    let mut c = client();
    // Java execute calls clientButton only from the var5 == 231 (BUTTON_OK)
    // arm; a BUTTON_TOGGLE with client code 205 must not arm the timer.
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 20);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_text: "Toggle".into(),
        client_code: 205,
        width: 190,
        height: 20,
        ..IfType::default()
    };
    let toggle_mut = IfTypeMut {
        button_type: ButtonType::BUTTON_TOGGLE,
        ..IfTypeMut::default()
    };
    bind_side(
        &mut c,
        1,
        vec![(root, IfTypeMut::default()), (toggle, toggle_mut)],
    );
    click_side(&mut c, 560, 210);
    // the TOGGLE_BUTTON arm sends IF_BUTTON but never calls clientButton
    assert_eq!(c.logout_timer, 0);
}

/// Bind `components` into the cache and `root` onto the active side tab (3).
fn bind_side(c: &mut Client, root: i32, components: Vec<(IfType, IfTypeMut)>) {
    c.ingame = true;
    c.side_icon[3] = root;
    c.active_icon = 3;
    for (com, m) in components {
        let id = com.id as usize;
        c.set_iface(id, com);
        c.set_iface_mut(id, m);
    }
}

/// Latch a left click at applet coords and fire it through the TS menu
/// path: `build_minimenu` populates the entries, `mouse_loop` fires the
/// last one (`doAction`).
fn click_side(c: &mut Client, x: i32, y: i32) {
    c.shell.mouse_x = x;
    c.shell.mouse_y = y;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.mouse_loop();
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
    let mut r = Renderer::new(false);
    let mut c = client();
    c.ingame = true;
    c.loginscreen = 2;
    c.redraw_frame = false;
    r.draw_area = PixMap::new(APPLET_W, APPLET_H);
    r.draw_area.fill(0x00ff00);
    c.logout();
    assert!(!c.ingame, "logout must leave the game state");
    assert_eq!(c.loginscreen, 0, "logout returns to the welcome screen");
    assert!(c.redraw_frame, "logout must force a full title redraw");
    // Paint teardown now runs in the renderer's first title draw (task 2b:
    // `logout` is sim-only; `title_screen_draw` drops `image_title2` and
    // cls's `draw_area` on the `!ingame && redraw_frame` gate). The draw
    // then plots the title art (a title jag may be present), so the
    // assertion is that no game-frame pixel survives, not an empty canvas.
    r.title_screen_draw(&mut c);
    assert!(
        r.draw_area.pixels.iter().all(|&p| p != 0x00ff00),
        "the title draw must clear draw_area so no game-frame pixel survives"
    );
}

#[test]
fn logout_then_title_draw_reallocates_regions() {
    let mut r = Renderer::new(false);
    let mut c = client();
    c.ingame = true;
    c.loginscreen = 2;
    c.redraw_frame = false;
    r.draw_area = PixMap::new(APPLET_W, APPLET_H);
    r.draw_area.fill(0x00ff00);
    c.logout();
    assert!(
        r.image_title2.is_none(),
        "fresh client has no title regions yet"
    );
    r.title_screen_draw(&mut c);
    assert!(
        r.image_title2.is_some(),
        "the next title draw must reallocate the title regions"
    );
    assert!(r.image_title0.is_some() && r.image_title1.is_some());
}

// --- Task 4b: `prepare_title` drops the game-frame areas ---
// Java `prepareTitle` (`Client.java` 1477-1511) nulls `super.drawArea` and
// the seven game areas before allocating the title regions, so a second
// login re-runs `prepareGame` instead of early-returning on a surviving
// `areaChatback`. Rust `prepare_title` must do the same (minus `draw_area`,
// which stays a single compositor PixMap).

#[test]
fn title_draw_drops_game_areas_so_relogin_rebuilds() {
    let mut r = Renderer::new(false);
    let mut c = client();
    c.ingame = true;
    c.loginscreen = 2;
    // A logged-in frame: all game areas alive, title regions gone.
    r.area_chat = Some(PixMap::new(479, 96));
    r.area_game = Some(PixMap::new(512, 334));
    r.area_map = Some(PixMap::new(172, 156));
    r.area_side = Some(PixMap::new(190, 261));
    r.area_backbase1 = Some(PixMap::new(496, 50));
    r.area_backbase2 = Some(PixMap::new(269, 37));
    r.area_backhmid1 = Some(PixMap::new(249, 45));
    c.logout();
    r.title_screen_draw(&mut c);
    assert!(
        r.area_chat.is_none(),
        "prepare_title must drop the game chat area (Java 1482)"
    );
    assert!(
        r.area_game.is_none(),
        "prepare_title must drop the game viewport area"
    );
    assert!(
        r.area_map.is_none() && r.area_side.is_none(),
        "prepare_title must drop the map/side areas"
    );
    assert!(
        r.image_title2.is_some(),
        "title regions must be reallocated"
    );
    // Second login: the next game draw rebuilds the frame and unloads the
    // title, instead of early-returning on the surviving `area_chat`.
    r.game_draw(&mut c);
    assert!(
        r.area_chat.is_some(),
        "prepare_game must rebuild the game areas after a relogin"
    );
    assert!(
        r.image_title2.is_none(),
        "prepare_game must unload the title regions"
    );
}
