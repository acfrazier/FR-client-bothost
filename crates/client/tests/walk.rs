// Task 7: walk-here pick + minimap click. `doAction(WALK)` arms World mouse
// picking with the applet click (b-4, c-4); `game_loop` consumes the ground
// answer into MOVE_GAMECLICK after the next render; `mouse_loop` builds the
// (undrawn) Cancel + Walk here minimenu for 3D-viewport clicks and auto-fires
// the top entry; `minimap_loop` converts a minimap click into
// MOVE_MINIMAPCLICK (Client.ts 2742).
use client::client::{Client, ClientConfig, ClientPlayer, MiniMenuAction};
use client::io::{ClientProt, Isaac};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// `doAction(WALK)` stores the click as applet mouseX/Y; the menu params feed
/// `World.updateMousePicking` scene-local (b-4, c-4), not the destination
/// tile. The walk packet is deferred to `game_loop`'s ground consume.
#[test]
fn walk_here_uses_picking_not_tiles_as_params() {
    let mut c = client();
    c.ingame = true;
    c.draw = true;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.menu_num_entries = 2;
    c.menu_action[1] = MiniMenuAction::WALK;
    c.menu_param_b[1] = 100; // mouse x
    c.menu_param_c[1] = 80; // mouse y
    c.doAction(1);
    assert!(c.world.click);
    assert_eq!(c.world.click_x, 96); // 100-4
    assert_eq!(c.world.click_y, 76); // 80-4
    // no inline walk packet: the pick is armed for the next render
    assert_eq!(c.out.pos, 0);
}

/// A left click inside the 146×151 minimap ring fires MOVE_MINIMAPCLICK.
/// The click (648, 83) maps to the map centre (relX/relY 0 with yaw/zoom 0),
/// so the walk is to the player's own tile (10,10).
#[test]
fn minimap_loop_type1_in_rect() {
    let mut c = client();
    c.ingame = true;
    let mut p = ClientPlayer::at(10, 10);
    p.x = 10 * 128 + 64;
    p.z = 10 * 128 + 64;
    c.local_player = Some(p);
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    c.shell.apply_mouse_down(1, 550 + 25 + 73, 4 + 4 + 75);
    c.shell.latch_click();
    c.minimap_loop();
    // MOVE_MINIMAPCLICK id 86: (86 + -621246914) & 0xff = 148
    let enc = (ClientProt::MOVE_MINIMAPCLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(enc, 148);
    assert_eq!(c.out.data()[0], enc);
    // size byte: 2 route bytes + 3 + the 14 minimap extras
    assert_eq!(c.out.data()[1], 1 + 1 + 3 + 14);
    assert_eq!(c.out.pos, 7 + 14);
    // the 14 extras: relX/Y 0, yaw 0, 57, angle 0, zoom 0, 89, x 1344, z 1344,
    // nearest 0, 63
    assert_eq!(
        &c.out.data()[7..21],
        &[0, 0, 0, 0, 57, 0, 0, 89, 5, 64, 5, 64, 0, 63]
    );
}

/// A left click outside the minimap ring is ignored.
#[test]
fn minimap_loop_outside_rect_is_ignored() {
    let mut c = client();
    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.shell.apply_mouse_down(1, 0, 0);
    c.shell.latch_click();
    c.minimap_loop();
    assert_eq!(c.out.pos, 0);
}

/// A left click in the 3D viewport builds the (undrawn) Cancel + Walk here
/// minimenu (`build_minimenu` runs each frame from `game_draw`; headless
/// tests call it directly) and `mouse_loop` auto-fires the top entry:
/// picking is armed with the click.
#[test]
fn mouse_loop_viewport_click_arms_picking() {
    let mut c = client();
    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.shell.apply_mouse_down(1, 100, 80);
    c.shell.latch_click();
    c.build_minimenu();
    c.mouse_loop();
    assert_eq!(c.menu_num_entries, 2);
    assert_eq!(c.menu_action[0], MiniMenuAction::CANCEL);
    assert_eq!(c.menu_action[1], MiniMenuAction::WALK);
    assert_eq!(c.menu_param_b[1], 100);
    assert_eq!(c.menu_param_c[1], 80);
    assert!(c.world.click);
    assert_eq!(c.world.click_x, 96);
    assert_eq!(c.world.click_y, 76);
}

/// A left click on the chrome (e.g. the side panel) must not walk.
#[test]
fn mouse_loop_chrome_click_does_not_walk() {
    let mut c = client();
    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.shell.apply_mouse_down(1, 600, 300);
    c.shell.latch_click();
    c.build_minimenu();
    c.mouse_loop();
    assert!(!c.world.click);
    assert_eq!(c.out.pos, 0);
}

/// `game_loop` consumes the frame's ground answer into MOVE_GAMECLICK and
/// resets `ground_x`.
#[test]
fn game_loop_consumes_ground_pick_into_move_gameclick() {
    let mut c = client();
    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    c.world.ground_x = 10;
    c.world.ground_z = 10;
    c.game_loop();
    assert_eq!(c.world.ground_x, -1);
    // MOVE_GAMECLICK id 207: (207 + -621246914) & 0xff = 13
    let enc = (ClientProt::MOVE_GAMECLICK.id.wrapping_add(-621246914)) as u8;
    // size 5 (2*1+3), ctrl 0, single absolute tile (10,10), no steps
    assert_eq!(&c.out.data()[..c.out.pos], &[enc, 5, 0, 0, 10, 0, 10]);
}
