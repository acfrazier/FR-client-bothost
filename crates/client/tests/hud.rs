// IF_SETICON / IF_SHOWICON / IF_OPENSIDE / IF_CLOSE handlers and the
// side-tab state they drive (side_icon / active_icon / side_modal_id /
// chat_modal_id), plus `draw_interface` drawing the active tab's interface
// into `area_side`. The /tmp cache has no packs, so `Client::new` falls back
// to `Cache::default()` and never touches the network (the /crc fetch on
// 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig, ClientPlayer};
use client::config::if_type::{ButtonType, ComponentType, IfType};
use client::config::{SeqType, VarpType};
use client::graphics::{Colour, Pix2D, PixMap};
use client::io::{ClientProt, JagFile, Packet};

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

#[test]
fn if_openside_sets_side_modal_id() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 50]); // com 50
    c.apply_if_openside(&mut p);
    assert_eq!(c.side_modal_id, 50);
    assert!(c.redraw_side && c.redraw_icons);
}

#[test]
fn if_close_clears_side_and_chat_modals() {
    let mut c = client();
    c.side_modal_id = 50;
    c.chat_modal_id = 7;
    c.apply_if_close();
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert!(c.redraw_side && c.redraw_icons && c.redraw_chat);
}

#[test]
fn if_openchat_sets_chat_and_closes_side() {
    let mut c = client();
    c.side_modal_id = 9;
    c.main_modal_id = 3;
    let mut p = Packet::new(vec![0, 40]);
    c.apply_if_openchat(&mut p);
    assert_eq!(c.chat_modal_id, 40);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.main_modal_id, -1);
    assert!(c.redraw_chat && c.redraw_side && c.redraw_icons);
    assert!(!c.resumed_pause_button);
}

#[test]
fn if_openmain_sets_main_and_closes_side_chat() {
    let mut c = client();
    c.side_modal_id = 9;
    c.chat_modal_id = 8;
    c.dialog_input_open = true;
    let mut p = Packet::new(vec![0, 70]);
    c.apply_if_openmain(&mut p);
    assert_eq!(c.main_modal_id, 70);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert!(!c.dialog_input_open);
}

#[test]
fn if_openmain_side_sets_both() {
    let mut c = client();
    c.chat_modal_id = 8;
    let mut p = Packet::new(vec![0, 10, 0, 20]);
    c.apply_if_openmain_side(&mut p);
    assert_eq!(c.main_modal_id, 10);
    assert_eq!(c.side_modal_id, 20);
    assert_eq!(c.chat_modal_id, -1);
}

#[test]
fn if_openoverlay_g2b_negative_clears() {
    let mut c = client();
    c.main_overlay_id = 5;
    let mut p = Packet::new(vec![0xff, 0xff]); // g2b -1
    c.apply_if_openoverlay(&mut p);
    assert_eq!(c.main_overlay_id, -1);
}

#[test]
fn if_openoverlay_g2b_positive_sets() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 12]);
    c.apply_if_openoverlay(&mut p);
    assert_eq!(c.main_overlay_id, 12);
}

#[test]
fn tut_flash_bounces_active_icon_when_same() {
    let mut c = client();
    c.active_icon = 3;
    c.apply_tut_flash(3);
    assert_eq!(c.tut_flash_icon, 3);
    assert_eq!(c.active_icon, 1); // TS: flash==3 → active=1
    assert!(c.redraw_side);
}

#[test]
fn tut_open_sets_tut_com_id() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 99]);
    c.apply_tut_open(&mut p);
    assert_eq!(c.tut_com_id, 99);
    assert!(c.redraw_chat);
}

#[test]
fn if_openside_clears_main_modal() {
    let mut c = client();
    c.main_modal_id = 3;
    c.chat_modal_id = 8;
    let mut p = Packet::new(vec![0, 50]);
    c.apply_if_openside(&mut p);
    assert_eq!(c.side_modal_id, 50);
    assert_eq!(c.main_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
}

#[test]
fn if_close_also_clears_main_modal() {
    let mut c = client();
    c.side_modal_id = 50;
    c.chat_modal_id = 7;
    c.main_modal_id = 3;
    c.dialog_input_open = true;
    c.apply_if_close();
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert_eq!(c.main_modal_id, -1);
    assert!(!c.dialog_input_open);
}

#[test]
fn draw_interface_text_writes_pixels() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("title").is_file() {
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
    c.game_draw(); // loads the p11/p12/b12/q8 fonts from the title jag

    // hand-built tree: a TYPE_LAYER root with a TYPE_TEXT child
    let layer = IfType {
        id: 1,
        r#type: ComponentType::TYPE_LAYER,
        width: 100,
        height: 50,
        children: Some(vec![2]),
        child_x: Some(vec![0]),
        child_y: Some(vec![0]),
        ..IfType::default()
    };
    let text = IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        text: "Hi".into(),
        colour: 0xffffff,
        font: 0, // p11
        ..IfType::default()
    };
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(text);

    let mut map = PixMap::new(100, 50);
    let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
    c.draw_interface(1, 0, 0, 0, &mut surface);
    assert!(map.pixels.iter().any(|&p| p != 0));
}

#[test]
fn iftype_keeps_graphic_name() {
    // the logout tab's red button is a TYPE_GRAPHIC sibling of its text
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("interface").is_file() {
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
    assert!(
        c.cache
            .ifaces
            .iter()
            .flatten()
            .any(|i| i.r#type == ComponentType::TYPE_GRAPHIC && !i.graphic_name.is_empty()),
        "the interface jag should hold a TYPE_GRAPHIC with a graphic_name"
    );
}

#[test]
fn draw_graphic_writes_pixels() {
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

    // hand-built tree: a TYPE_LAYER root with a TYPE_GRAPHIC child using a
    // real media sprite ("miscgraphics,0" is present in the pack)
    let layer = IfType {
        id: 1,
        r#type: ComponentType::TYPE_LAYER,
        width: 190,
        height: 261,
        children: Some(vec![2]),
        child_x: Some(vec![0]),
        child_y: Some(vec![0]),
        ..IfType::default()
    };
    let graphic = IfType {
        id: 2,
        r#type: ComponentType::TYPE_GRAPHIC,
        graphic_name: "miscgraphics,0".into(),
        ..IfType::default()
    };
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(graphic);

    let mut map = PixMap::new(190, 261);
    let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
    c.draw_interface(1, 0, 0, 0, &mut surface);
    assert!(
        map.pixels.iter().any(|&p| p != 0),
        "the TYPE_GRAPHIC sprite should plot into the surface"
    );
}

#[test]
fn draw_side_text_component_writes_pixels() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("interface").is_file() {
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
    let before = c.area_side.as_ref().unwrap().pixels.clone();

    // no server packets have run, so no tab is bound: inject a text tree on
    // the active tab (3) so draw_side has an interface to draw
    let layer = IfType {
        id: 1,
        r#type: ComponentType::TYPE_LAYER,
        width: 190,
        height: 261,
        children: Some(vec![2]),
        child_x: Some(vec![0]),
        child_y: Some(vec![0]),
        ..IfType::default()
    };
    let text = IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        text: "Logout".into(),
        colour: 0xffffff,
        ..IfType::default()
    };
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(text);
    c.side_icon[3] = 1;
    c.redraw_side = true;
    c.game_draw();

    let after = c.area_side.as_ref().unwrap();
    assert!(
        before.iter().zip(&after.pixels).any(|(a, b)| a != b),
        "draw_side must draw the injected text into area_side"
    );
}

#[test]
fn add_chat_shifts_and_redraws() {
    let mut c = client();
    c.add_chat(0, "hello", "");
    assert_eq!(c.chat_text[0], "hello");
    assert!(c.redraw_chat);
    c.add_chat(0, "world", "");
    assert_eq!(c.chat_text[0], "world");
    assert_eq!(c.chat_text[1], "hello");
}

#[test]
fn message_game_plain_is_type_0() {
    let mut c = client();
    let mut p = Packet::new({
        let mut v = Vec::new();
        // gjstr "Welcome" + newline
        v.extend(b"Welcome\n");
        v
    });
    c.apply_message_game(&mut p);
    assert_eq!(c.chat_text[0], "Welcome");
    assert_eq!(c.chat_type[0], 0);
}

#[test]
fn message_game_tradereq_is_type_4() {
    let mut c = client();
    let mut p = Packet::new({
        let mut v = Vec::new();
        v.extend(b"Zezima:tradereq:\n");
        v
    });
    c.apply_message_game(&mut p);
    assert_eq!(c.chat_type[0], 4);
    assert_eq!(c.chat_text[0], "wishes to trade with you.");
    assert_eq!(c.chat_username[0], "Zezima");
}

#[test]
fn message_game_duelreq_is_type_8() {
    let mut c = client();
    let mut p = Packet::new({
        let mut v = Vec::new();
        v.extend(b"Zezima:duelreq:\n");
        v
    });
    c.apply_message_game(&mut p);
    assert_eq!(c.chat_type[0], 8);
    assert_eq!(c.chat_text[0], "wishes to duel with you.");
    assert_eq!(c.chat_username[0], "Zezima");
}

#[test]
fn chat_enter_clears_input_and_echoes_without_player_name() {
    let mut c = client();
    c.ingame = true;
    c.chat_input = "hello".into();
    c.shell.apply_key(true, 10, 10);
    c.handle_chat_input();
    assert!(c.chat_input.is_empty());
    assert_eq!(c.chat_text[0], "hello");
}

#[test]
fn chat_input_appends_prints_and_backspaces() {
    let mut c = client();
    c.ingame = true;
    c.shell.apply_key(true, 0, 'h' as i32);
    c.shell.apply_key(true, 0, 'i' as i32);
    c.handle_chat_input();
    assert_eq!(c.chat_input, "hi");
    c.shell.apply_key(true, 0, 8);
    c.handle_chat_input();
    assert_eq!(c.chat_input, "h");
}

#[test]
fn chat_input_command_sends_client_cheat() {
    let mut c = client();
    c.ingame = true;
    for ch in b"::ping" {
        c.shell.apply_key(true, 0, *ch as i32);
    }
    c.handle_chat_input();
    assert_eq!(c.chat_input, "::ping");
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(c.chat_input, "");
    // CLIENT_CHEAT: p1_enc(224) + p1(len-2+1) + pjstr("ping")
    assert_eq!(c.out.data()[0] as i32, ClientProt::CLIENT_CHEAT.id & 0xff);
    assert_eq!(c.out.data()[1], (6 - 2 + 1) as u8);
    assert_eq!(&c.out.data()[2..7], b"ping\n");
}

#[test]
fn chat_input_public_sends_message_public() {
    let mut c = client();
    c.ingame = true;
    let mut player = ClientPlayer::at(1, 1);
    player.name = Some("Bob".into());
    c.local_player = Some(player);
    for ch in b"hello" {
        c.shell.apply_key(true, 0, *ch as i32);
    }
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(c.chat_input, "");
    // MESSAGE_PUBLIC: p1_enc(253) p1(0 len) p1(colour) p1(effect) pjstr("hello")
    assert_eq!(c.out.data()[0] as i32, ClientProt::MESSAGE_PUBLIC.id & 0xff);
    assert_eq!(c.out.data()[1], 8); // len: colour + effect + "hello\n"
    assert_eq!(c.out.data()[2], 0); // colour
    assert_eq!(c.out.data()[3], 0); // effect
    assert_eq!(&c.out.data()[4..10], b"hello\n");
    // own message echoes into the chat as type 2 with the player name
    assert_eq!(c.chat_text[0], "hello");
    assert_eq!(c.chat_type[0], 2);
    assert_eq!(c.chat_username[0], "Bob");
}

#[test]
fn draw_chat_empty_history_does_not_panic() {
    let mut c = client();
    c.ingame = true;
    c.game_draw();
    c.redraw_chat = true;
    c.game_draw();
}

#[test]
fn draw_chat_renders_type0_line_and_input() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("title").is_file() {
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
    c.game_draw(); // loads p12 from the title jag
    c.add_chat(0, "hello", "");
    c.chat_input = "abc".into();
    c.redraw_chat = true;
    c.game_draw();
    // type-0 line is black text; the input line is blue text
    let chat = c.area_chat.as_ref().unwrap();
    assert!(chat.pixels.contains(&Colour::BLACK));
    assert!(chat.pixels.contains(&Colour::BLUE));
}

/// draw a single TYPE_TEXT child under a fixed layer; returns the pixmap.
fn draw_text(c: &mut Client, text: &IfType) -> PixMap {
    let layer = IfType {
        id: 1,
        r#type: ComponentType::TYPE_LAYER,
        width: 100,
        height: 50,
        children: Some(vec![2]),
        child_x: Some(vec![0]),
        child_y: Some(vec![0]),
        ..IfType::default()
    };
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(text.clone());
    let mut map = PixMap::new(100, 50);
    let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
    c.draw_interface(1, 0, 0, 0, &mut surface);
    map
}

fn text_com(text: &str, scripts: Option<Vec<Vec<i32>>>) -> IfType {
    IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        text: text.into(),
        scripts,
        colour: 0xffffff,
        font: 0, // p11
        ..IfType::default()
    }
}

#[test]
fn get_if_var_stat_effective() {
    let mut c = client();
    c.stat_effective_level[0] = 12;
    let com = IfType {
        scripts: Some(vec![vec![1, 0, 0]]), // opcode 1 stat_effective, skill 0, halt
        ..IfType::default()
    };
    assert_eq!(c.get_if_var(&com, 0), Some(12));
}

#[test]
fn get_if_var_pushvar() {
    let mut c = client();
    c.var = vec![0, 0, 0, 42];
    let com = IfType {
        scripts: Some(vec![vec![5, 3, 0]]), // opcode 5 pushvar 3, halt
        ..IfType::default()
    };
    assert_eq!(c.get_if_var(&com, 0), Some(42));
}

#[test]
fn get_if_var_missing_scripts_is_none() {
    let c = client();
    let com = IfType {
        scripts: None,
        ..IfType::default()
    };
    assert_eq!(c.get_if_var(&com, 0), None);
}

#[test]
fn get_if_var_out_of_range_script_id_is_none() {
    let c = client();
    let com = IfType {
        scripts: Some(vec![vec![0]]),
        ..IfType::default()
    };
    assert_eq!(c.get_if_var(&com, 1), None);
}

#[test]
fn draw_interface_substitutes_percent1() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("title").is_file() {
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
    c.game_draw(); // loads the p11/p12/b12/q8 fonts from the title jag
    c.stat_effective_level[0] = 12;

    let literal = draw_text(&mut c, &text_com("%1", None));
    let substituted = draw_text(&mut c, &text_com("%1", Some(vec![vec![1, 0, 0]])));
    assert_ne!(
        substituted.pixels, literal.pixels,
        "a %1 script must substitute, not draw the literal text"
    );
    assert_eq!(
        substituted.pixels,
        draw_text(&mut c, &text_com("12", None)).pixels,
        "substituted %1 must render exactly like the literal value"
    );

    // inf: values >= 999_999_999 render as '*'
    let star = draw_text(&mut c, &text_com("%1", Some(vec![vec![20, 999_999_999, 0]])));
    assert_eq!(
        star.pixels,
        draw_text(&mut c, &text_com("*", None)).pixels,
        "a %1 of 999_999_999 must render as '*'"
    );
}

// ---- sidebar left-click IF_BUTTON (Task 4) ----

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

/// The bytes written by the outgoing packet buffer.
fn out_bytes(c: &Client) -> &[u8] {
    &c.out.data()[..c.out.pos]
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

/// A non-layer child with the given button type and rect.
fn side_button(id: i32, button_type: i32, x: i32, y: i32, width: i32, height: i32) -> IfType {
    IfType {
        id,
        r#type: ComponentType::TYPE_RECT,
        button_type,
        x,
        y,
        width,
        height,
        ..IfType::default()
    }
}

#[test]
fn side_click_ok_button_writes_if_button() {
    let mut c = client();
    // the panel is blitted at (553, 205); a click at (560, 210) is local (7, 5)
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 20);
    let button = side_button(2, ButtonType::BUTTON_OK, 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    click_side(&mut c, 560, 210);
    // random is None at Client::new, so p1_enc writes the plain opcode
    assert_eq!(out_bytes(&c), &[9, 0, 2]); // IF_BUTTON (id 9) + child 2 big-endian
}

#[test]
fn side_click_close_button_sends_close_modal() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let button = side_button(2, ButtonType::BUTTON_CLOSE, 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    click_side(&mut c, 560, 210);
    // CLOSE_MODAL (opcode 51, length 0): the opcode only, no p2 payload
    assert_eq!(out_bytes(&c), &[51]);
    assert_eq!(c.side_modal_id, -1);
}

#[test]
fn side_click_toggle_flips_var_and_redraws() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_TOGGLE,
        width: 190,
        height: 20,
        scripts: Some(vec![vec![5, 7, 0]]), // scripts[0][0] == 5: varp 7
        ..IfType::default()
    };
    c.var = vec![0, 0, 0, 0, 0, 0, 0, 1];
    bind_side(&mut c, 1, vec![root, toggle]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var[7], 0);
    assert!(c.redraw_side);
}

#[test]
fn side_click_toggle_without_script_keeps_var() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let toggle = side_button(2, ButtonType::BUTTON_TOGGLE, 0, 0, 190, 20);
    c.var = vec![1];
    bind_side(&mut c, 1, vec![root, toggle]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var, vec![1]);
    assert!(!c.redraw_side);
}

#[test]
fn side_click_toggle_applies_varp_clientcode() {
    let mut c = client();
    // varp 0 carries the music clientcode (3): flipping the toggle must
    // change the volume through clientVar, not just var/redraw_side
    c.cache.varps = vec![VarpType { clientcode: 3 }];
    c.var = vec![1];
    c.midi_volume = -800;
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_TOGGLE,
        width: 190,
        height: 20,
        scripts: Some(vec![vec![5, 0, 0]]), // varp 0
        ..IfType::default()
    };
    bind_side(&mut c, 1, vec![root, toggle]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var[0], 0);
    assert_eq!(c.midi_volume, 0); // clientcode 3 value 0 → +0 dB
    assert!(c.midi_active);
}

#[test]
fn side_click_select_sets_var_when_different() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let select = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_SELECT,
        width: 190,
        height: 20,
        scripts: Some(vec![vec![5, 7, 0]]),
        script_operand: Some(vec![42]),
        ..IfType::default()
    };
    c.var = vec![0, 0, 0, 0, 0, 0, 0, 1];
    bind_side(&mut c, 1, vec![root, select]);
    c.redraw_side = false;
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var[7], 42);
    assert!(c.redraw_side);

    // a matching var still sends the packet but neither writes nor redraws
    c.out.pos = 0;
    c.redraw_side = false;
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var[7], 42);
    assert!(!c.redraw_side);
}

#[test]
fn side_click_requires_left_button_in_panel() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let button = side_button(2, ButtonType::BUTTON_OK, 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    // a right click is not a button press
    c.shell.apply_mouse_down(2, 560, 210);
    c.shell.latch_click();
    c.handle_side_if_clicks();
    assert_eq!(out_bytes(&c), &[]);
    // below the panel (466 is the bottom edge) the click is ignored
    click_side(&mut c, 560, 470);
    assert_eq!(out_bytes(&c), &[]);
    // in the panel the button fires
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
}

#[test]
fn side_click_skips_non_button_first_child() {
    let mut c = client();
    let root = side_layer(1, vec![2, 3], vec![0, 0], vec![0, 0], 190, 261);
    let text = IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        width: 190,
        height: 40,
        ..IfType::default()
    };
    let button = side_button(3, ButtonType::BUTTON_OK, 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, text, button]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 3]);
}

#[test]
fn side_click_uses_side_modal_when_open() {
    let mut c = client();
    let tab_root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let tab_button = side_button(2, ButtonType::BUTTON_OK, 0, 0, 190, 20);
    let modal_root = side_layer(4, vec![5], vec![0], vec![0], 190, 261);
    let modal_button = side_button(5, ButtonType::BUTTON_CLOSE, 0, 0, 190, 20);
    bind_side(
        &mut c,
        1,
        vec![tab_root, tab_button, modal_root, modal_button],
    );
    c.side_modal_id = 4;
    c.chat_modal_id = 9;
    click_side(&mut c, 560, 210);
    // the modal tree's CLOSE child fires closeModal: CLOSE_MODAL only, and
    // both modals clear (hitting the tab tree's OK child would send IF_BUTTON)
    assert_eq!(out_bytes(&c), &[51]);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert!(c.redraw_side && c.redraw_icons && c.redraw_chat);
}

#[test]
fn side_click_layer_recurse_with_clamped_scroll() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    // scroll_pos 999 clamps to scroll_height - height = 20, so the button
    // at local y 30 covers 10..30: a click at local y 12 hits, y 35 misses
    let scroller = IfType {
        id: 2,
        r#type: ComponentType::TYPE_LAYER,
        width: 190,
        height: 100,
        scroll_height: 120,
        scroll_pos: 999,
        children: Some(vec![3]),
        child_x: Some(vec![0]),
        child_y: Some(vec![30]),
        ..IfType::default()
    };
    let button = side_button(3, ButtonType::BUTTON_OK, 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, scroller, button]);
    click_side(&mut c, 558, 217); // local (5, 12)
    assert_eq!(out_bytes(&c), &[9, 0, 3]);
    c.out.pos = 0;
    click_side(&mut c, 558, 240); // local (5, 35): below the scrolled rect
    assert_eq!(out_bytes(&c), &[]);
}

#[test]
fn side_click_real_logout_text_sends_if_button() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("interface").is_file() {
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

    // the "Click here to logout" control: a BUTTON_OK text child of the
    // logout interface's root layer (id/offset come from the real pack)
    let Some(logout) = c
        .cache
        .ifaces
        .iter()
        .flatten()
        .find(|com| com.text == "Click here to logout")
    else {
        return; // pack layout changed; the hand-built tests still cover this
    };
    assert_eq!(logout.button_type, ButtonType::BUTTON_OK);
    let logout_id = logout.id;
    let layer_id = logout.layer_id;
    let (mut click_x, mut click_y) = (0, 0);
    let mut placed = false;
    if let Some(layer) = c
        .cache
        .ifaces
        .get(layer_id as usize)
        .and_then(|o| o.as_ref())
    {
        if let (Some(children), Some(child_x), Some(child_y)) =
            (&layer.children, &layer.child_x, &layer.child_y)
        {
            for i in 0..children.len() {
                if children[i] == logout_id {
                    // same formula as the handler's child rect: parent
                    // offset + child offset + the child's own x/y
                    click_x = child_x[i] + layer.x + logout.x;
                    click_y = child_y[i] + layer.y + logout.y;
                    placed = true;
                    break;
                }
            }
        }
    }
    assert!(placed, "the logout text must sit under a layer");

    c.side_icon[13] = layer_id;
    c.active_icon = 13;
    c.shell.apply_mouse_down(1, 553 + click_x, 205 + click_y);
    c.shell.latch_click();
    c.handle_side_if_clicks();
    // random is None at Client::new, so p1_enc writes the plain opcode
    let expected = [
        ClientProt::IF_BUTTON.id as u8,
        (logout_id >> 8) as u8,
        (logout_id & 0xff) as u8,
    ];
    assert_eq!(&c.out.data()[..c.out.pos], &expected);
}

#[test]
fn iftype_unpack_keeps_inv_background_names() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("interface").is_file() {
        return;
    }
    let jag = JagFile::new(std::fs::read(format!("{cache}/interface")).unwrap());
    let ifaces = IfType::unpack(&jag);
    assert!(
        ifaces.iter().flatten().any(|c| c
            .inv_background_name
            .as_ref()
            .is_some_and(|v| v.iter().any(|n| n.is_some()))),
        "a TYPE_INV component should keep its inv-background sprite names"
    );
}

#[test]
fn inv_number_formats_k_and_m() {
    let c = client();
    assert_eq!(c.inv_number(999), "999");
    assert_eq!(c.inv_number(99_999), "99999");
    assert_eq!(c.inv_number(100_000), "100K");
    assert_eq!(c.inv_number(150_000), "150K");
    assert_eq!(c.inv_number(12_000_000), "12M");
}

// ---- main/chat modal drawing and clicks (slice 3 Task 2) ----

#[test]
fn draw_chat_uses_chat_modal_instead_of_lines() {
    let mut c = client();
    c.ingame = true;
    c.game_draw(); // prepare_game allocates area_chat
    c.chat_text[0] = "hello".into();
    c.chat_modal_id = 1;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 479;
    layer.height = 96;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut rect = IfType::default();
    rect.r#type = ComponentType::TYPE_RECT;
    rect.fill = true;
    rect.width = 40;
    rect.height = 10;
    rect.colour = 0x00ff00;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(rect);
    c.redraw_chat = true;
    c.game_draw();
    let chat = c.area_chat.as_ref().expect("prepare_game allocates area_chat");
    assert!(
        chat.pixels.iter().any(|&p| p & 0xffffff == 0x00ff00),
        "chat modal TYPE_RECT must plot into area_chat"
    );
}

#[test]
fn draw_chat_modal_clears_stale_chat_lines() {
    // TS 11125-11146: chatback plots first, then the modal iface replaces
    // the chat lines — a modal opened after chat text must not keep the old
    // line pixels (regression: the modal branch used to early-return).
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
    c.game_draw(); // chatback + fonts
    c.add_chat(0, "hello", ""); // a black type-0 line (redraw_chat set)
    c.game_draw();
    let chat = c.area_chat.as_ref().unwrap();
    let black_spots: Vec<usize> = chat
        .pixels
        .iter()
        .enumerate()
        .filter(|(_, &p)| p & 0xffffff == 0x000000)
        .map(|(i, _)| i)
        .collect();
    assert!(!black_spots.is_empty(), "the type-0 line must leave black pixels");
    // open a chat modal over the same area
    c.chat_modal_id = 1;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 479;
    layer.height = 96;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut rect = IfType::default();
    rect.r#type = ComponentType::TYPE_RECT;
    rect.fill = true;
    rect.width = 40;
    rect.height = 10;
    rect.colour = 0x00ff00;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(rect);
    c.redraw_chat = true;
    c.game_draw();
    let chat = c.area_chat.as_ref().unwrap();
    for i in black_spots {
        assert_ne!(
            chat.pixels[i] & 0xffffff,
            0x000000,
            "chatback must clear the stale chat line at pixel {i}"
        );
    }
    assert!(
        chat.pixels.iter().any(|&p| p & 0xffffff == 0x00ff00),
        "the chat modal must still plot on top"
    );
}

#[test]
fn main_modal_click_sends_if_button() {
    let mut c = client();
    c.main_modal_id = 1;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 512;
    layer.height = 334;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![10]);
    layer.child_y = Some(vec![10]);
    let mut btn = IfType::default();
    btn.r#type = ComponentType::TYPE_TEXT;
    btn.button_type = ButtonType::BUTTON_OK;
    btn.width = 80;
    btn.height = 20;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(btn);
    c.shell.apply_mouse_down(1, 4 + 10 + 5, 4 + 10 + 5);
    c.shell.latch_click();
    c.handle_main_if_clicks();
    assert_eq!(c.out.data()[0], ClientProt::IF_BUTTON.id as u8);
    assert_eq!(c.out.data()[1], 0);
    assert_eq!(c.out.data()[2], 2);
}

#[test]
fn mouse_loop_skips_walk_when_main_modal_open() {
    let mut c = client();
    c.main_modal_id = 1;
    c.shell.apply_mouse_down(1, 100, 100);
    c.shell.latch_click();
    let pos = c.out.pos;
    c.mouse_loop();
    assert_eq!(c.out.pos, pos);
}

#[test]
fn animate_interface_advances_model_frame() {
    let mut c = client();
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut model = IfType::default();
    model.r#type = ComponentType::TYPE_MODEL;
    model.model_anim = 0;
    model.anim_frame = 0;
    model.anim_cycle = 0;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(model);
    c.cache.seqs.resize(1, SeqType::default());
    c.cache.seqs[0].num_frames = 2;
    c.cache.seqs[0].frames = Some(vec![0, 0]);
    c.cache.seqs[0].iframes = Some(vec![-1, -1]);
    c.cache.seqs[0].delay = Some(vec![1, 1]);
    c.cache.seqs[0].loops = 2;
    assert!(c.animate_interface(1, 2));
    assert_eq!(c.cache.ifaces[2].as_ref().unwrap().anim_frame, 1);
}

#[test]
fn if_anim_reset_zeros_nested_layer_child() {
    let mut c = client();
    let mut root = IfType::default();
    root.r#type = ComponentType::TYPE_LAYER;
    root.children = Some(vec![2]);
    let mut inner = IfType::default();
    inner.id = 2; // recursion follows child.id, so the layer must know it
    inner.r#type = ComponentType::TYPE_LAYER;
    inner.children = Some(vec![3]);
    let mut model = IfType::default();
    model.r#type = ComponentType::TYPE_MODEL;
    model.anim_frame = 4;
    model.anim_cycle = 9;
    c.cache.ifaces.resize(4, None);
    c.cache.ifaces[1] = Some(root);
    c.cache.ifaces[2] = Some(inner);
    c.cache.ifaces[3] = Some(model);
    c.if_anim_reset(1);
    assert_eq!(c.cache.ifaces[3].as_ref().unwrap().anim_frame, 0);
    assert_eq!(c.cache.ifaces[3].as_ref().unwrap().anim_cycle, 0);
}

#[test]
fn game_draw_redraws_side_and_chat_when_modal_anims() {
    let mut c = client();
    // separate model trees for the side and chat modals; world_update_num 2
    // must advance both frames through the game_draw animate triggers
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut model = IfType::default();
    model.r#type = ComponentType::TYPE_MODEL;
    model.model_anim = 0;
    let mut chat_layer = IfType::default();
    chat_layer.r#type = ComponentType::TYPE_LAYER;
    chat_layer.children = Some(vec![4]);
    chat_layer.child_x = Some(vec![0]);
    chat_layer.child_y = Some(vec![0]);
    let mut chat_model = IfType::default();
    chat_model.r#type = ComponentType::TYPE_MODEL;
    chat_model.model_anim = 0;
    c.cache.ifaces.resize(5, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(model);
    c.cache.ifaces[3] = Some(chat_layer);
    c.cache.ifaces[4] = Some(chat_model);
    c.cache.seqs.resize(1, SeqType::default());
    c.cache.seqs[0].num_frames = 2;
    c.cache.seqs[0].frames = Some(vec![0, 0]);
    c.cache.seqs[0].iframes = Some(vec![-1, -1]);
    c.cache.seqs[0].delay = Some(vec![1, 1]);
    c.cache.seqs[0].loops = 2;
    c.side_modal_id = 1;
    c.chat_modal_id = 3;
    c.world_update_num = 2;
    c.redraw_side = false;
    c.redraw_chat = false;
    c.game_draw();
    assert_eq!(c.cache.ifaces[2].as_ref().unwrap().anim_frame, 1);
    assert_eq!(c.cache.ifaces[4].as_ref().unwrap().anim_frame, 1);
}

#[test]
fn draw_icons_flash_sends_tut_clickside_and_clears() {
    let mut c = client();
    c.tut_flash_icon = 3;
    c.active_icon = 3;
    c.redraw_icons = true;
    c.game_draw();
    assert_eq!(c.tut_flash_icon, -1);
    assert_eq!(c.out.data()[0], ClientProt::TUT_CLICKSIDE.id as u8);
    assert_eq!(c.out.data()[1], 3);
}

#[test]
fn draw_icons_blinks_flashing_tab() {
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
    c.side_icon[3] = 1;
    c.active_icon = 0; // flash stays (flash tab != active tab)
    c.tut_flash_icon = 3;
    c.game_draw();
    // blink on half-cycle (loopCycle % 20 < 10): the tab plots
    c.loop_cycle = 5;
    c.redraw_icons = true;
    c.game_draw();
    let on = c.area_backhmid1.as_ref().unwrap().pixels.clone();
    // blink off half-cycle (loopCycle % 20 >= 10): the tab is hidden
    c.loop_cycle = 15;
    c.redraw_icons = true;
    c.game_draw();
    let off = c.area_backhmid1.as_ref().unwrap().pixels.clone();
    assert_ne!(on, off, "the flashing tab must blink with loop_cycle");
}
