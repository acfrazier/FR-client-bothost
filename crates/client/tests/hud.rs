// IF_SETICON / IF_SHOWICON / IF_OPENSIDE / IF_CLOSE handlers and the
// side-tab state they drive (side_icon / active_icon / side_modal_id /
// chat_modal_id), plus `draw_interface` drawing the active tab's interface
// into `area_side`. The /tmp cache has no packs, so `Client::new` falls back
// to `Cache::default()` and never touches the network (the /crc fetch on
// 127.0.0.1 is refused instantly).
use std::sync::Arc;

use client::client::{Client, ClientConfig, ClientPlayer};
use client::config::if_type::{ButtonType, ComponentType, IfType};
use client::config::{ObjType, SeqType, VarpType};
use client::graphics::{Colour, Pix2D, Pix8, PixMap};
use client::io::{ClientProt, JagFile, Packet};
use client::wordfilter::WordPack;

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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(text);

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
        c.ifaces
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(graphic);

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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(text);
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
    // the echo is sentence-cased (WordFilter stays identity without wordenc)
    assert_eq!(c.chat_text[0], "Hello");
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
    // MESSAGE_PUBLIC: p1_enc(253) p1(0 len) p1(colour) p1(effect) WordPack("hello")
    assert_eq!(c.out.data()[0] as i32, ClientProt::MESSAGE_PUBLIC.id & 0xff);
    assert_eq!(c.out.data()[2], 0); // colour
    assert_eq!(c.out.data()[3], 0); // effect
    let packed_len = c.out.data()[1] as usize - 2; // size minus colour+effect
    let mut tail = Packet::new(c.out.data()[4..4 + packed_len].to_vec());
    // WordPack.unpack of a packed "hello" is "Hello " (trailing carry
    // space, oracle 61 bb 40); the echo is the sentence-cased text.
    assert_eq!(WordPack::unpack(&mut tail, packed_len), "Hello ");
    // own message echoes into the chat as type 2 with the player name
    assert_eq!(c.chat_text[0], "Hello");
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

// ---- mod_icons: staff crowns in chat (slice 6 Task 4) ----

/// The most common non-zero palette colour in a sprite (its main fill),
/// used to detect the sprite's pixels in a rendered area. 0 when the sprite
/// has no drawn pixels.
fn sprite_fill_colour(sprite: &Pix8) -> i32 {
    let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    for &idx in &sprite.data {
        if idx != 0 {
            let rgb = sprite.bpal.get((idx as u8) as usize).copied().unwrap_or(0);
            if rgb != 0 {
                *counts.entry(rgb).or_insert(0) += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|&(_, n)| n).map(|(c, _)| c).unwrap_or(0)
}

/// A client whose `media` sprites and fonts are loaded (`prepare_game`).
fn chat_client(cache: &str) -> Client {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache.into(),
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.game_draw();
    c
}

/// Draw one chat line and return the `area_chat` pixels.
fn chat_line_pixels(cache: &str, r#type: i32, sender: &str) -> Vec<i32> {
    let mut c = chat_client(cache);
    c.add_chat(r#type, "hello", sender);
    c.redraw_chat = true;
    c.game_draw();
    c.area_chat.as_ref().unwrap().pixels.clone()
}

#[test]
fn prepare_game_depacks_mod_icons() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let c = chat_client(&cache);
    assert!(
        c.mod_icons[0].is_some(),
        "mod_icons[0] (gold @cr1@ crown) must depack from the media jag"
    );
    assert!(
        c.mod_icons[1].is_some(),
        "mod_icons[1] (silver @cr2@ crown) must depack from the media jag"
    );
}

#[test]
fn draw_chat_plots_cr1_crown() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let crown_colour = {
        let c = chat_client(&cache);
        sprite_fill_colour(c.mod_icons[0].as_ref().expect("mod_icons[0] depacked"))
    };
    assert_ne!(crown_colour, 0, "the gold crown sprite must have drawn pixels");
    // control: the same line without the @cr1@ prefix
    let control = chat_line_pixels(&cache, 2, "Mod");
    let rendered = chat_line_pixels(&cache, 2, "@cr1@Mod");
    let n_control = control.iter().filter(|&&p| p == crown_colour).count();
    let n_crown = rendered.iter().filter(|&&p| p == crown_colour).count();
    assert!(
        n_crown > n_control,
        "the @cr1@ render must add gold crown pixels ({crown_colour:#06x}) to area_chat \
         (control {n_control}, crown {n_crown})"
    );
}

#[test]
fn draw_chat_plots_cr2_crown_for_private() {
    // types 3/7 with split_private_chat 0 draw the private line in the
    // chatbox: the silver crown plots after the "From" label.
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let silver_colour = {
        let c = chat_client(&cache);
        sprite_fill_colour(c.mod_icons[1].as_ref().expect("mod_icons[1] depacked"))
    };
    assert_ne!(silver_colour, 0, "the silver crown sprite must have drawn pixels");
    let control = chat_line_pixels(&cache, 7, "Eve");
    let rendered = chat_line_pixels(&cache, 7, "@cr2@Eve");
    let n_control = control.iter().filter(|&&p| p == silver_colour).count();
    let n_crown = rendered.iter().filter(|&&p| p == silver_colour).count();
    assert!(
        n_crown > n_control,
        "the @cr2@ private render must add silver crown pixels ({silver_colour:#06x}) \
         to area_chat (control {n_control}, crown {n_crown})"
    );
}

#[test]
fn draw_private_messages_plots_cr1_crown() {
    // split private chat draws into area_game at y = 329 - line*13.
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = chat_client(&cache);
    c.scene_state = 2; // game_draw_main (and its overlays) run only in-scene
    c.split_private_chat = 1;
    let gold_colour = {
        let c = chat_client(&cache);
        sprite_fill_colour(c.mod_icons[0].as_ref().expect("mod_icons[0] depacked"))
    };
    assert_ne!(gold_colour, 0, "the gold crown sprite must have drawn pixels");
    c.add_chat(3, "hello", "Eve");
    c.redraw_chat = true;
    c.game_draw();
    let control = c.area_game.as_ref().unwrap().pixels.clone();
    c.add_chat(3, "hello", "@cr1@Eve");
    c.redraw_chat = true;
    c.game_draw();
    let rendered = c.area_game.as_ref().unwrap().pixels.clone();
    let n_control = control.iter().filter(|&&p| p == gold_colour).count();
    let n_crown = rendered.iter().filter(|&&p| p == gold_colour).count();
    assert!(
        n_crown > n_control,
        "the split-PM @cr1@ render must add gold crown pixels ({gold_colour:#06x}) \
         to area_game (control {n_control}, crown {n_crown})"
    );
}

#[test]
fn draw_chat_social_overlay_draws_header_and_input() {
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
    c.game_draw(); // loads the b12 font from the title jag
    c.add_chat(0, "hidden by the overlay", "");
    c.social_input_open = true;
    c.social_input_header = "Enter name of friend to add to list".into();
    c.social_input = "bob".into();
    c.redraw_chat = true;
    c.game_draw();
    // TS 11133-11135: header black at (239,40), input dark blue at (239,60)
    let chat = c.area_chat.as_ref().unwrap();
    assert!(chat.pixels.contains(&Colour::BLACK));
    assert!(chat.pixels.contains(&Colour::DARKBLUE));
}

#[test]
fn draw_chat_dialog_overlay_draws_header_and_input() {
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
    c.game_draw(); // loads the b12 font from the title jag
    c.add_chat(0, "hidden by the overlay", "");
    c.dialog_input_open = true;
    c.dialog_input = "42".into();
    c.redraw_chat = true;
    c.game_draw();
    // TS 11136-11138: "Enter amount:" black at (239,40), input dark blue
    // at (239,60) — the plain chat input line would be Colour::BLUE
    let chat = c.area_chat.as_ref().unwrap();
    assert!(chat.pixels.contains(&Colour::BLACK));
    assert!(chat.pixels.contains(&Colour::DARKBLUE));
}

#[test]
fn draw_reboot_timer_overlay_draws_system_update() {
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
    c.scene_state = 2; // game_draw_main (and its overlays) run only in-scene
    c.game_draw(); // loads the p12 font from the title jag
    c.reboot_timer = 100; // 2 seconds
    c.game_draw();
    // TS 4901-4911: "System update in: M:SS" yellow at (4,329) in area_game
    let game = c.area_game.as_ref().unwrap();
    assert!(game.pixels.contains(&Colour::YELLOW));
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(text.clone());
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

// ---- sidebar left-click buttons (Tasks 4/5) ----
// Clicks flow through `build_minimenu` + `mouse_loop`, which fire the last
// menu entry's `doAction` (IF_BUTTON/CLOSE/TOGGLE/SELECT/PAUSE arms).

/// Bind `components` into the cache and `root` onto the active side tab (3).
fn bind_side(c: &mut Client, root: i32, components: Vec<IfType>) {
    c.ingame = true;
    c.side_icon[3] = root;
    c.active_icon = 3;
    let max = components.iter().map(|com| com.id).max().unwrap_or(0) as usize;
    c.ifaces.resize(max + 1, None);
    for com in components {
        let id = com.id as usize;
        c.ifaces[id] = Some(com);
    }
}

/// Latch a left click at applet coords and fire it through the TS menu
/// path: `build_minimenu` populates the entries, `mouse_loop` fires the
/// last one (`doAction`). `handle_*_if_clicks` are no-ops (double-dispatch).
fn click_side(c: &mut Client, x: i32, y: i32) {
    c.shell.mouse_x = x;
    c.shell.mouse_y = y;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.mouse_loop();
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

/// A non-layer child with the given button type and rect. `text` is the
/// button label `build_minimenu` requires before it pushes the option
/// (TS 9785-9789).
fn side_button(
    id: i32,
    button_type: i32,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> IfType {
    IfType {
        id,
        r#type: ComponentType::TYPE_RECT,
        button_type,
        button_text: text.into(),
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
    let button = side_button(2, ButtonType::BUTTON_OK, "OK", 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    click_side(&mut c, 560, 210);
    // random is None at Client::new, so p1_enc writes the plain opcode
    assert_eq!(out_bytes(&c), &[9, 0, 2]); // IF_BUTTON (id 9) + child 2 big-endian
}

#[test]
fn side_click_close_button_sends_close_modal() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let button = side_button(2, ButtonType::BUTTON_CLOSE, "", 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    click_side(&mut c, 560, 210);
    // CLOSE_MODAL (opcode 51, length 0): the opcode only, no p2 payload
    assert_eq!(out_bytes(&c), &[51]);
    assert_eq!(c.side_modal_id, -1);
}

#[test]
fn close_modal_clears_resumed_pause_button() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let button = side_button(2, ButtonType::BUTTON_CLOSE, "", 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    c.resumed_pause_button = true;
    click_side(&mut c, 560, 210);
    assert_eq!(c.side_modal_id, -1);
    assert!(!c.resumed_pause_button);
}

#[test]
fn side_click_toggle_flips_var_and_redraws() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_TOGGLE,
        button_text: "Toggle".into(),
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
    let toggle = side_button(2, ButtonType::BUTTON_TOGGLE, "Toggle", 0, 0, 190, 20);
    c.var = vec![1];
    bind_side(&mut c, 1, vec![root, toggle]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var, vec![1]);
    // the TS doAction tail always redraws after a button action, but the
    // var is untouched without the scripts[0][0] == 5 preamble
    assert!(c.redraw_side);
}

#[test]
fn side_click_toggle_applies_varp_clientcode() {
    let mut c = client();
    // varp 0 carries the music clientcode (3): flipping the toggle must
    // change the volume through clientVar, not just var/redraw_side
    Arc::get_mut(&mut c.cache).unwrap().varps = vec![VarpType { clientcode: 3 }];
    c.var = vec![1];
    c.midi_volume = -800;
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let toggle = IfType {
        id: 2,
        r#type: ComponentType::TYPE_RECT,
        button_type: ButtonType::BUTTON_TOGGLE,
        button_text: "Music".into(),
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
        button_text: "Select".into(),
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

    // a matching var still sends the packet and the TS doAction tail
    // still redraws, but the var is untouched
    c.out.pos = 0;
    c.redraw_side = false;
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 2]);
    assert_eq!(c.var[7], 42);
    assert!(c.redraw_side);
}

#[test]
fn side_click_requires_left_button_in_panel() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let button = side_button(2, ButtonType::BUTTON_OK, "OK", 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, button]);
    // a right click opens the menu instead of pressing the button
    c.shell.mouse_x = 560;
    c.shell.mouse_y = 210;
    c.build_minimenu();
    c.shell.apply_mouse_down(2, 560, 210);
    c.shell.latch_click();
    c.mouse_loop();
    assert_eq!(out_bytes(&c), &[]);
    assert!(c.is_menu_open);
    c.is_menu_open = false;
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
    let button = side_button(3, ButtonType::BUTTON_OK, "OK", 0, 0, 190, 20);
    bind_side(&mut c, 1, vec![root, text, button]);
    click_side(&mut c, 560, 210);
    assert_eq!(out_bytes(&c), &[9, 0, 3]);
}

#[test]
fn side_click_uses_side_modal_when_open() {
    let mut c = client();
    let tab_root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    let tab_button = side_button(2, ButtonType::BUTTON_OK, "OK", 0, 0, 190, 20);
    let modal_root = side_layer(4, vec![5], vec![0], vec![0], 190, 261);
    let modal_button = side_button(5, ButtonType::BUTTON_CLOSE, "", 0, 0, 190, 20);
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
fn side_click_layer_recurse_with_scrolled_child() {
    let mut c = client();
    let root = side_layer(1, vec![2], vec![0], vec![0], 190, 261);
    // a scroller at scroll_pos 20 puts the child at local y 30-20 = 10,
    // covering 10..30: a click at local y 12 hits, y 35 misses
    // (`addComponentOptions` uses the raw scroll_pos, TS 9653)
    let scroller = IfType {
        id: 2,
        r#type: ComponentType::TYPE_LAYER,
        width: 190,
        height: 100,
        scroll_height: 120,
        scroll_pos: 20,
        children: Some(vec![3]),
        child_x: Some(vec![0]),
        child_y: Some(vec![30]),
        ..IfType::default()
    };
    let button = side_button(3, ButtonType::BUTTON_OK, "OK", 0, 0, 190, 20);
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
    // the menu path: build_minimenu walks the side tree, mouse_loop fires
    // the last entry (the logout text's IF_BUTTON option)
    c.shell.mouse_x = 553 + click_x;
    c.shell.mouse_y = 205 + click_y;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 553 + click_x, 205 + click_y);
    c.shell.latch_click();
    c.mouse_loop();
    // random is None at Client::new, so p1_enc writes the plain opcode
    let expected = [
        ClientProt::IF_BUTTON.id as u8,
        (logout_id >> 8) as u8,
        (logout_id & 0xff) as u8,
    ];
    assert_eq!(&c.out.data()[..c.out.pos], &expected);
    // the CC_LOGOUT client code arms the logout timer through clientButton
    assert_eq!(c.logout_timer, 250);
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

// ---- scrollbars (Task 5) ----

#[test]
fn draw_scrollbar_fills_track() {
    let mut c = client();
    let mut pixels = vec![0i32; 16 * 77];
    let mut surface = Pix2D::with_pixels(&mut pixels, 16, 77);
    c.draw_scrollbar(&mut surface, 0, 0, 0, 200, 77);
    // track is fill_rect(x, y+16, 16, height-32, 0x23201b); the grip (rows
    // 16..33 at scroll_y 0) and its lowlight hlines stay above row 50.
    let idx = (16 * 50) as usize; // a pixel inside the track
    assert_eq!(pixels[idx] & 0xffffff, 0x23201b);
}

#[test]
fn do_scrollbar_up_arrow_decreases_pos() {
    let mut c = client();
    c.scroll_cycle = 1;
    let mut com = IfType::default();
    com.r#type = ComponentType::TYPE_LAYER;
    com.scroll_height = 200;
    com.height = 77;
    com.scroll_pos = 40;
    com.width = 100;
    c.ifaces.resize(1, None);
    c.ifaces[0] = Some(com);
    // x,y inside the top 16×16 at left=463 top=0 → pass x=463 y=0
    c.do_scrollbar(463, 0, 200, 77, true, 463, 0, 0);
    assert_eq!(c.ifaces[0].as_ref().unwrap().scroll_pos, 36); // - scroll_cycle*4
    assert!(c.redraw_side);
}

#[test]
fn game_draw_chat_scrollbar_up_arrow_steps_scroll_pos() {
    let mut c = client();
    c.ingame = true;
    // TS 3948-3967: with no chat modal, a held mouse on the chat
    // scrollbar's top arrow (applet 480,357 → local 463,0) steps
    // `chat_scroll_pos` through `chat_interface`.
    c.shell.mouse_button = 1;
    c.shell.mouse_x = 480;
    c.shell.mouse_y = 357;
    c.game_draw();
    assert_eq!(c.chat_scroll_pos, 1);
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

#[test]
fn nice_number_groups_and_colours() {
    let c = client();
    assert_eq!(c.nice_number(1), " 1");
    assert_eq!(c.nice_number(1234), " @cya@1K @whi@(1,234)");
    assert_eq!(c.nice_number(12_345_678), " @gre@12 million @whi@(12,345,678)");
}

#[test]
fn draw_interface_inv_text_writes_obj_name() {
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
    c.game_draw(); // fonts
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 200;
    layer.height = 50;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut inv = IfType::default();
    inv.r#type = ComponentType::TYPE_INV_TEXT;
    inv.width = 1;
    inv.height = 1;
    inv.link_obj_type = Some(vec![1]); // obj id 0 + 1
    inv.link_obj_number = Some(vec![1]);
    inv.colour = 0xffffff;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(inv);
    if c.cache.objs.is_empty() {
        return;
    }
    let mut pixels = vec![0i32; 200 * 50];
    let mut surface = Pix2D::with_pixels(&mut pixels, 200, 50);
    c.draw_interface(1, 0, 0, 0, &mut surface);
    assert!(pixels.iter().any(|&p| p != 0), "inv-text should plot the obj name");
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(rect);
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(rect);
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
    btn.id = 2;
    btn.r#type = ComponentType::TYPE_TEXT;
    btn.button_type = ButtonType::BUTTON_OK;
    btn.button_text = "Continue".into();
    btn.width = 80;
    btn.height = 20;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(btn);
    // the menu path: build_minimenu pushes the button's IF_BUTTON option,
    // mouse_loop fires the last entry (build_minimenu needs the live
    // pointer at the click position)
    c.shell.mouse_x = 4 + 10 + 5;
    c.shell.mouse_y = 4 + 10 + 5;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 4 + 10 + 5, 4 + 10 + 5);
    c.shell.latch_click();
    c.mouse_loop();
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
    // the main modal eats the viewport: build_minimenu walks the modal
    // (no component options in an empty cache), so there is no WALK entry
    // for `mouse_loop` to fire
    c.build_minimenu();
    let pos = c.out.pos;
    c.mouse_loop();
    assert_eq!(c.out.pos, pos);
    assert!(!c.world.click);
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(model);
    Arc::get_mut(&mut c.cache).unwrap().seqs.resize(1, SeqType::default());
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].num_frames = 2;
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].frames = Some(vec![0, 0]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].iframes = Some(vec![-1, -1]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].delay = Some(vec![1, 1]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].loops = 2;
    assert!(c.animate_interface(1, 2));
    assert_eq!(c.ifaces[2].as_ref().unwrap().anim_frame, 1);
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
    c.ifaces.resize(4, None);
    c.ifaces[1] = Some(root);
    c.ifaces[2] = Some(inner);
    c.ifaces[3] = Some(model);
    c.if_anim_reset(1);
    assert_eq!(c.ifaces[3].as_ref().unwrap().anim_frame, 0);
    assert_eq!(c.ifaces[3].as_ref().unwrap().anim_cycle, 0);
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
    c.ifaces.resize(5, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(model);
    c.ifaces[3] = Some(chat_layer);
    c.ifaces[4] = Some(chat_model);
    Arc::get_mut(&mut c.cache).unwrap().seqs.resize(1, SeqType::default());
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].num_frames = 2;
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].frames = Some(vec![0, 0]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].iframes = Some(vec![-1, -1]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].delay = Some(vec![1, 1]);
    Arc::get_mut(&mut c.cache).unwrap().seqs[0].loops = 2;
    c.side_modal_id = 1;
    c.chat_modal_id = 3;
    c.world_update_num = 2;
    c.redraw_side = false;
    c.redraw_chat = false;
    c.game_draw();
    assert_eq!(c.ifaces[2].as_ref().unwrap().anim_frame, 1);
    assert_eq!(c.ifaces[4].as_ref().unwrap().anim_frame, 1);
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

// ---- pointer hover over*ComId (Task 6) ----

/// A TYPE_LAYER with one child list (child offsets and layer size).
fn hover_layer(id: i32, width: i32, height: i32, children: Vec<i32>, child_x: Vec<i32>, child_y: Vec<i32>) -> IfType {
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

#[test]
fn update_if_pointer_sets_over_side_and_redraws() {
    let mut c = client();
    c.side_modal_id = 1;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut child = IfType::default();
    child.id = 2;
    child.r#type = ComponentType::TYPE_TEXT;
    child.width = 50;
    child.height = 20;
    child.colour_over = 0xff0000;
    child.over_layer_id = -1;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(child);
    c.shell.mouse_x = 553 + 5;
    c.shell.mouse_y = 205 + 5;
    c.update_if_pointer();
    assert_eq!(c.over_side_com_id, 2);
    assert!(c.redraw_side);
}

#[test]
fn hidden_layer_draws_when_hovered() {
    let mut c = client();
    c.over_side_com_id = 1;
    let mut layer = IfType::default();
    layer.id = 1;
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.hide = true;
    layer.width = 20;
    layer.height = 20;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut rect = IfType::default();
    rect.r#type = ComponentType::TYPE_RECT;
    rect.fill = true;
    rect.width = 20;
    rect.height = 20;
    rect.colour = 0x112233;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(rect);
    let mut pixels = vec![0i32; 20 * 20];
    let mut surface = Pix2D::with_pixels(&mut pixels, 20, 20);
    c.draw_interface(1, 0, 0, 0, &mut surface);
    assert_eq!(pixels[0] & 0xffffff, 0x112233);
}

#[test]
fn update_if_pointer_sets_over_main() {
    let mut c = client();
    c.main_modal_id = 1;
    let layer = hover_layer(1, 512, 334, vec![2], vec![0], vec![0]);
    let mut child = IfType::default();
    child.id = 2;
    child.r#type = ComponentType::TYPE_TEXT;
    child.width = 200;
    child.height = 200;
    child.colour_over = 0xff0000;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(child);
    c.shell.mouse_x = 100;
    c.shell.mouse_y = 100;
    c.update_if_pointer();
    assert_eq!(c.over_main_com_id, 2);
}

#[test]
fn update_if_pointer_sets_over_chat_and_redraws_chat() {
    let mut c = client();
    c.chat_modal_id = 1;
    let layer = hover_layer(1, 479, 96, vec![2], vec![0], vec![0]);
    let mut child = IfType::default();
    child.id = 2;
    child.r#type = ComponentType::TYPE_TEXT;
    child.width = 190;
    child.height = 96;
    child.colour_over = 0xff0000;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(child);
    c.shell.mouse_x = 200;
    c.shell.mouse_y = 400;
    c.update_if_pointer();
    assert_eq!(c.over_chat_com_id, 2);
    assert!(c.redraw_chat);
}

#[test]
fn update_if_pointer_resets_over_side_when_pointer_leaves() {
    let mut c = client();
    c.over_side_com_id = 2;
    c.redraw_side = false;
    c.shell.mouse_x = 750;
    c.shell.mouse_y = 480;
    c.update_if_pointer();
    assert_eq!(c.over_side_com_id, 0);
    assert!(c.redraw_side);
}

#[test]
fn hovered_rect_uses_colour_over() {
    let mut c = client();
    let layer = hover_layer(1, 20, 20, vec![2], vec![0], vec![0]);
    let mut rect = IfType::default();
    rect.id = 2;
    rect.r#type = ComponentType::TYPE_RECT;
    rect.fill = true;
    rect.width = 20;
    rect.height = 20;
    rect.colour = 0x112233;
    rect.colour_over = 0xff0000;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(rect);

    let mut plain = vec![0i32; 20 * 20];
    let mut plain_surface = Pix2D::with_pixels(&mut plain, 20, 20);
    c.draw_interface(1, 0, 0, 0, &mut plain_surface);
    assert_eq!(plain[0] & 0xffffff, 0x112233);

    c.over_side_com_id = 2;
    let mut hovered = vec![0i32; 20 * 20];
    let mut hovered_surface = Pix2D::with_pixels(&mut hovered, 20, 20);
    c.draw_interface(1, 0, 0, 0, &mut hovered_surface);
    assert_eq!(hovered[0] & 0xffffff, 0xff0000);
}

#[test]
fn hover_walk_steps_scrollable_layer_scrollbar() {
    let mut c = client();
    c.side_modal_id = 1;
    // the scroller sits at local (0, 30) of the side panel (553, 205)
    let root = hover_layer(1, 190, 261, vec![2], vec![0], vec![30]);
    let mut scroller = IfType::default();
    scroller.id = 2;
    scroller.r#type = ComponentType::TYPE_LAYER;
    scroller.width = 100;
    scroller.height = 100;
    scroller.scroll_height = 120;
    scroller.children = Some(vec![3]);
    scroller.child_x = Some(vec![0]);
    scroller.child_y = Some(vec![0]);
    let mut button = IfType::default();
    button.id = 3;
    button.r#type = ComponentType::TYPE_RECT;
    button.width = 100;
    button.height = 20;
    c.ifaces.resize(4, None);
    c.ifaces[1] = Some(root);
    c.ifaces[2] = Some(scroller);
    c.ifaces[3] = Some(button);
    // the scroller's scrollbar sits at left = 553 + 100 = 653, top =
    // 205 + 30 = 235; its down arrow is x 653..669, y 319..335.
    c.scroll_cycle = 1;
    c.shell.mouse_x = 660;
    c.shell.mouse_y = 327;
    c.update_if_pointer();
    assert_eq!(c.ifaces[2].as_ref().unwrap().scroll_pos, 4); // + scroll_cycle*4
    assert!(c.redraw_side);
}

#[test]
fn type_inv_hover_sets_hovered_slot_on_empty_slot() {
    let mut c = client();
    c.side_modal_id = 1;
    let root = hover_layer(1, 190, 261, vec![2], vec![0], vec![0]);
    let mut inv = IfType::default();
    inv.id = 2;
    inv.r#type = ComponentType::TYPE_INV;
    inv.width = 1; // one column
    inv.height = 1; // one row
    inv.margin_x = 0;
    inv.margin_y = 0;
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(root);
    c.ifaces[2] = Some(inv);
    c.shell.mouse_x = 553 + 5;
    c.shell.mouse_y = 205 + 5;
    c.update_if_pointer();
    assert_eq!(c.hovered_slot, 0);
    assert_eq!(c.hovered_slot_com_id, 2);
}

#[test]
fn resumed_pause_button_text_is_please_wait() {
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

    let cont = IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        text: "Hello".into(),
        button_type: ButtonType::BUTTON_CONTINUE,
        colour: 0xffffff,
        font: 0, // p11
        ..IfType::default()
    };
    c.resumed_pause_button = true;
    let waiting = draw_text(&mut c, &cont);
    c.resumed_pause_button = false;
    let plain = draw_text(&mut c, &cont);
    let literal = draw_text(&mut c, &text_com("Please wait...", None));
    assert_eq!(
        waiting.pixels, literal.pixels,
        "BUTTON_CONTINUE + resumed_pause_button must render 'Please wait...'"
    );
    assert_ne!(
        plain.pixels, literal.pixels,
        "without resumed_pause_button the button text stays 'Hello'"
    );
}

#[test]
fn active_text_uses_text2() {
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

    // an active (comparator-satisfied) text with text2 must render text2
    let active = IfType {
        id: 2,
        r#type: ComponentType::TYPE_TEXT,
        text: "Hello".into(),
        text2: "Over".into(),
        colour: 0xffffff,
        colour2: 0xffffff,
        font: 0, // p11
        scripts: Some(vec![vec![0]]), // halt with acc 0
        script_comparator: Some(vec![1]),
        script_operand: Some(vec![0]),
        ..IfType::default()
    };
    assert_eq!(
        draw_text(&mut c, &active).pixels,
        draw_text(&mut c, &text_com("Over", None)).pixels,
        "an active text must render text2, not text"
    );
}

// ---- inventory obj-drag (Task 8) ----

#[test]
fn swap_slots_exchanges_type_and_count() {
    let mut com = IfType {
        link_obj_type: Some(vec![10, 20]),
        link_obj_number: Some(vec![1, 5]),
        ..IfType::default()
    };
    com.swap_slots(0, 1);
    assert_eq!(com.link_obj_type.as_ref().unwrap(), &vec![20, 10]);
    assert_eq!(com.link_obj_number.as_ref().unwrap(), &vec![5, 1]);
}

#[test]
fn obj_drag_release_sends_inv_buttond() {
    let mut c = client();
    c.side_modal_id = 1;
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
    let inv = IfType {
        id: 2,
        layer_id: 1,
        r#type: ComponentType::TYPE_INV,
        obj_swap: true,
        width: 2,
        height: 1,
        margin_x: 0,
        margin_y: 0,
        link_obj_type: Some(vec![5, 0]),
        link_obj_number: Some(vec![1, 0]),
        ..IfType::default()
    };
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(inv);
    // the slot holds obj id 5, so `build_minimenu` can name it for the
    // Examine entry (the drag-start target)
    if c.cache.objs.len() < 5 {
        Arc::get_mut(&mut c.cache).unwrap().objs.resize(5, ObjType::default());
    }
    Arc::get_mut(&mut c.cache).unwrap().objs[4].name = "Rune".into();
    // grab slot 0: build the menu (the last entry is the occupied slot's
    // Examine, in the drag-start action set), then left-click through
    // `mouse_loop` like the real frame
    c.shell.mouse_x = 553 + 16;
    c.shell.mouse_y = 205 + 16;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 553 + 16, 205 + 16);
    c.shell.latch_click();
    c.mouse_loop();
    assert_eq!(c.obj_drag_area, 2);
    assert_eq!(c.obj_drag_slot, 0);
    // move past 5px and hold for 5 cycles
    c.shell.mouse_x = 553 + 16 + 40;
    c.shell.mouse_y = 205 + 16;
    c.shell.mouse_button = 1;
    for _ in 0..5 {
        c.handle_obj_drag();
    }
    // drop on slot 1
    c.shell.mouse_x = 553 + 32 + 16;
    c.shell.mouse_y = 205 + 16;
    c.shell.mouse_button = 0;
    c.handle_obj_drag();
    assert_eq!(c.obj_drag_area, 0);
    assert_eq!(
        c.ifaces[2].as_ref().unwrap().link_obj_type.as_ref().unwrap()[1],
        5
    );
    // INV_BUTTOND (id 93): p1_enc(93) p2(com) p2(src) p2(dst) p1(mode),
    // p2 big-endian — dst 1 is bytes [5]=0, [6]=1
    assert_eq!(c.out.data()[0], ClientProt::INV_BUTTOND.id as u8);
    assert_eq!(c.out.data()[1], 0);
    assert_eq!(c.out.data()[2], 2);
    assert_eq!(c.out.data()[3], 0);
    assert_eq!(c.out.data()[4], 0);
    assert_eq!(c.out.data()[5], 0);
    assert_eq!(c.out.data()[6], 1);
    assert_eq!(c.out.data()[7], 0);
}

#[test]
fn obj_drag_quick_release_fires_last_entry_same_slot_drop_does_not() {
    let mut c = client();
    c.side_modal_id = 1;
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
    let inv = IfType {
        id: 2,
        layer_id: 1,
        r#type: ComponentType::TYPE_INV,
        obj_swap: true,
        width: 2,
        height: 1,
        margin_x: 0,
        margin_y: 0,
        link_obj_type: Some(vec![5, 0]),
        link_obj_number: Some(vec![1, 0]),
        ..IfType::default()
    };
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(layer);
    c.ifaces[2] = Some(inv);
    // the slot holds obj id 5, so `build_minimenu` can name it for the
    // Examine entry (the drag-start target)
    if c.cache.objs.len() < 5 {
        Arc::get_mut(&mut c.cache).unwrap().objs.resize(5, ObjType::default());
    }
    Arc::get_mut(&mut c.cache).unwrap().objs[4].name = "Rune".into();
    Arc::get_mut(&mut c.cache).unwrap().objs[4].desc = String::new();
    // grab slot 0 via the full click path: build the menu, then left-click
    // through `mouse_loop`
    c.shell.mouse_x = 553 + 16;
    c.shell.mouse_y = 205 + 16;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 553 + 16, 205 + 16);
    c.shell.latch_click();
    c.mouse_loop();
    assert_eq!(c.obj_drag_area, 2);
    // quick release (no grab threshold): the TS 2291-2296 else-if fires
    // the last menu entry — the slot's Examine (OP_HELD6) — which chats;
    // no INV_BUTTOND and no item move.
    c.shell.mouse_x = 553 + 16;
    c.shell.mouse_y = 205 + 16;
    c.shell.mouse_button = 0;
    c.handle_obj_drag();
    assert_eq!(c.obj_drag_area, 0);
    assert_eq!(c.out.pos, 0, "a quick release must not send INV_BUTTOND");
    assert_eq!(
        c.ifaces[2].as_ref().unwrap().link_obj_type.as_ref().unwrap(),
        &vec![5, 0],
        "a quick release must not move the item"
    );
    assert_eq!(
        c.chat_text[0], "It's a Rune.",
        "a quick release fires the last-entry Examine"
    );
    // mark the chat so a second doAction is detectable
    c.chat_text[0] = "sentinel".into();
    // pick the item up again, hold past the threshold for 5 cycles, and
    // release over its own slot: the threshold branch re-walks and finds
    // the same slot, so neither INV_BUTTOND nor the last-entry doAction
    // fires (the else-if is the else of the threshold if, TS 2246-2296).
    c.shell.mouse_x = 553 + 16;
    c.shell.mouse_y = 205 + 16;
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 553 + 16, 205 + 16);
    c.shell.latch_click();
    c.mouse_loop();
    assert_eq!(c.obj_drag_area, 2);
    c.shell.mouse_x = 553 + 16 + 6; // past the ±5px grab threshold
    c.shell.mouse_y = 205 + 16;
    c.shell.mouse_button = 1;
    for _ in 0..5 {
        c.handle_obj_drag();
    }
    c.shell.mouse_button = 0;
    c.handle_obj_drag();
    assert_eq!(c.obj_drag_area, 0);
    assert_eq!(c.out.pos, 0, "a same-slot drop must not send INV_BUTTOND");
    assert_eq!(
        c.ifaces[2].as_ref().unwrap().link_obj_type.as_ref().unwrap(),
        &vec![5, 0],
        "a same-slot drop must not move the item"
    );
    assert_eq!(
        c.chat_text[0], "sentinel",
        "a same-slot drop must not fire the last-entry doAction"
    );
}
