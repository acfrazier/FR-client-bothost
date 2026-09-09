//! Offline production-frame proof. All fonts, sprites and interfaces are synthetic;
//! no cache-dependent early returns or GPU adapter are involved.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::if_type::{ButtonType, ComponentType, IfType, IfTypeMut};
use client::graphics::{Pix8, PixFont, PixMap};
use client::render::Renderer;
use std::sync::Arc;

const BACK: i32 = 0xabc123;
const TUTORIAL: i32 = 0x123456;
const MODAL: i32 = 0x654321;

// One pixel per non-space glyph, advance two, baseline one below the pixel.
// The oracle below is independent of PixFont's measurement/drawing functions.
fn font() -> PixFont {
    PixFont {
        char_mask: vec![vec![1]; 256],
        char_mask_width: [1; 256],
        char_mask_height: [1; 256],
        char_offset_x: [0; 256],
        char_offset_y: [0; 256],
        char_advance: [2; 256],
        height: 1,
    }
}

fn interface(c: &mut Client, id: i32, colour: i32) {
    c.set_iface(
        id as usize,
        IfType {
            id,
            r#type: ComponentType::TYPE_LAYER,
            width: 479,
            height: 96,
            children: Some(vec![id + 1]),
            child_x: Some(vec![0]),
            child_y: Some(vec![0]),
            ..Default::default()
        },
    );
    c.set_iface_mut(id as usize, IfTypeMut::default());
    c.set_iface(
        (id + 1) as usize,
        IfType {
            id: id + 1,
            r#type: ComponentType::TYPE_RECT,
            width: 479,
            height: 96,
            fill: true,
            button_text: "Synthetic action".into(),
            ..Default::default()
        },
    );
    c.set_iface_mut(
        (id + 1) as usize,
        IfTypeMut {
            colour,
            button_type: ButtonType::BUTTON_OK,
            ..Default::default()
        },
    );
}

fn fixture(revision: ClientRevision) -> (Client, Renderer) {
    let cache_dir = format!(
        "{}/../../target/tutorial-presentation-absent-cache-{}",
        env!("CARGO_MANIFEST_DIR"),
        std::process::id()
    );
    assert!(
        !std::path::Path::new(&cache_dir).exists(),
        "no incidental cache"
    );
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir,
            members: true,
            lowmem: false,
        },
        revision,
    );
    c.ingame = true;
    c.scene_state = 1; // Normal loading-frame path, no world/cache/scene raster needed.
    interface(&mut c, 10, TUTORIAL);
    interface(&mut c, 20, MODAL);
    let mut r = Renderer::new(false);
    r.area_chat = Some(PixMap::new(479, 96));
    r.area_game = Some(PixMap::new(512, 334));
    r.area_game.as_mut().unwrap().pixels.fill(0x345678);
    let mut media = client::render::Media::empty();
    media.b12 = Some(font());
    media.p12 = Some(font());
    let mut back = Pix8::new(479, 96, vec![0, BACK]);
    back.data.fill(1);
    media.chatback = Some(back);
    r.media = Arc::new(media);
    r.game_draw(&mut c); // Warm up ordinary frame dirtiness before each assertion.
    (c, r)
}

fn expected_prompt(message: &str, cue: &str) -> Vec<i32> {
    let mut pixels = vec![BACK; 479 * 96];
    for (text, baseline, colour) in [(message, 40, 0), (cue, 60, 128)] {
        let start = 239 - text.len();
        for (i, ch) in text.bytes().enumerate() {
            if ch != b' ' {
                pixels[(baseline - 1) * 479 + start + 2 * i] = colour;
            }
        }
    }
    pixels
}

fn assert_chat(r: &Renderer, expected: &[i32]) {
    let chat = r.area_chat.as_ref().expect("synthetic chat surface");
    assert_eq!(chat.pixels.len(), expected.len());
    for (i, (&actual, &want)) in chat.pixels.iter().zip(expected).enumerate() {
        assert_eq!(actual, want, "chat pixel ({}, {})", i % 479, i / 479);
        assert_eq!(
            r.draw_area.pixels[(357 + i / 479) * 765 + 17 + i % 479],
            want,
            "composited chat pixel ({}, {})",
            i % 479,
            i / 479
        );
    }
}

#[test]
fn pending_replaces_tutorial_on_chatback_with_primary_colours() {
    let (mut c, mut r) = fixture(ClientRevision::R289);
    c.tut_com_id = 10;
    c.tut_com_message = Some("Message".into());
    c.redraw_chat = true;
    r.game_draw(&mut c);
    assert_chat(&r, &expected_prompt("Message", "Click to continue"));
}

#[test]
fn pending_redraws_each_frame_without_other_dirty_regions() {
    let (mut c, mut r) = fixture(ClientRevision::R289);
    c.tut_com_message = Some("Message".into());
    c.redraw_chat = true;
    r.game_draw(&mut c);
    for message in ["Message", "", "Replacement"] {
        c.tut_com_message = Some(message.into());
        assert!(!c.redraw_chat);
        assert!(!c.redraw_frame && !c.redraw_side && !c.redraw_icons && !c.redraw_chat_mode);
        // Corrupt only the chat surface, making a missed redraw
        // observable even when the pending text itself has not changed.
        r.area_chat.as_mut().unwrap().pixels.fill(0xeeeeee);
        let outside = r.draw_area.pixels.clone();
        let scene = r.area_game.as_ref().unwrap().pixels.clone();
        r.game_draw(&mut c);
        assert_chat(&r, &expected_prompt(message, "Click to continue"));
        assert!(!c.redraw_chat, "backend consumed the dirty flag");
        assert_eq!(r.area_game.as_ref().unwrap().pixels, scene);
        for (i, &before) in outside.iter().enumerate() {
            let (x, y) = (i % 765, i / 765);
            if !(17..496).contains(&x) || !(357..453).contains(&y) {
                assert_eq!(r.draw_area.pixels[i], before, "non-chat pixel changed");
            }
        }
    }
}

// Actual shell capture and production pre-menu handler subsequence; no
// assignment of a post-ack click and no direct do_action shortcut.
fn left(c: &mut Client) {
    c.shell.apply_mouse_down(1, 560, 210);
    c.shell.latch_click();
    c.handle_obj_drag();
    c.handle_tab_clicks();
    c.handle_side_if_clicks();
    c.handle_main_if_clicks();
    c.handle_chat_if_clicks();
    c.chat_mode_loop();
    c.handle_chat_input();
    c.mouse_loop();
    c.minimap_loop();
}

#[test]
fn pending_empty_and_nonempty_replace_all_underlying_modes_then_ack_restores() {
    for (tutorial, modal) in [(10, -1), (-1, -1), (-1, 20), (10, 20)] {
        for message in ["Message", ""] {
            let (mut c, mut r) = fixture(ClientRevision::R289);
            c.tut_com_id = tutorial;
            c.chat_modal_id = modal;
            c.redraw_chat = true;
            r.game_draw(&mut c);
            let underlying = r.area_chat.as_ref().unwrap().pixels.clone();
            if modal != -1 {
                assert_chat(&r, &vec![MODAL; 479 * 96]);
            } else if tutorial != -1 {
                assert_chat(&r, &vec![TUTORIAL; 479 * 96]);
            } else {
                assert_ne!(underlying, expected_prompt("", "Click to continue"));
                assert_eq!(underlying[77 * 479], 0); // ordinary chat separator
            }
            c.tut_com_message = Some(message.into());
            assert!(!c.redraw_chat); // no arrival/IF dirty flag to rescue scheduling
            r.game_draw(&mut c);
            assert_chat(&r, &expected_prompt(message, "Click to continue"));

            interface(&mut c, 30, 0x112233);
            c.side_icon[3] = 30;
            c.active_icon = 3;
            c.shell.mouse_x = 560;
            c.shell.mouse_y = 210;
            c.build_minimenu();
            assert_eq!(c.menu_num_entries, 2);
            left(&mut c);
            assert_eq!(c.tut_com_message, None);
            assert_eq!(c.shell.mouse_click_button, 0);
            assert_eq!(c.out.pos, 0, "ack must not send an action");
            assert!(c.redraw_chat);
            r.game_draw(&mut c);
            assert_chat(&r, &underlying);
            c.build_minimenu();
            left(&mut c);
            // Primary 289 IF_BUTTON 86, p2 component (J11234-11242).
            assert_eq!(&c.out.data()[..c.out.pos], &[86, 0, 31]);
        }
    }
}

#[test]
fn social_then_amount_still_outrank_pending_message() {
    for revision in [ClientRevision::R289, ClientRevision::R274] {
        let (mut c, mut r) = fixture(revision);
        c.tut_com_id = 10;
        c.chat_modal_id = 20;
        c.tut_com_message = Some("Message".into());
        c.social_input_open = true;
        c.social_input_header = "Social".into();
        c.social_input = "Friend".into();
        c.dialog_input_open = true;
        c.dialog_input = "42".into();
        c.redraw_chat = true;
        r.game_draw(&mut c);
        assert_chat(&r, &expected_prompt("Social", "Friend*"));
        c.social_input_open = false;
        c.redraw_chat = true;
        r.game_draw(&mut c);
        assert_chat(&r, &expected_prompt("Enter amount:", "42*"));
        assert_eq!(c.tut_com_message.as_deref(), Some("Message"));
    }
}

#[test]
fn legacy_274_keeps_base_interface_order_and_no_pending_frame_dirtiness() {
    for (tutorial, modal) in [(10, -1), (-1, -1), (-1, 20), (10, 20)] {
        let (mut c, mut r) = fixture(ClientRevision::R274);
        c.tut_com_id = tutorial;
        c.chat_modal_id = modal;
        c.redraw_chat = true;
        r.game_draw(&mut c);
        let underlying = r.area_chat.as_ref().unwrap().pixels.clone();
        if modal != -1 {
            assert_chat(&r, &vec![MODAL; 479 * 96]);
        } else if tutorial != -1 {
            assert_chat(&r, &vec![TUTORIAL; 479 * 96]);
        } else {
            assert_eq!(underlying[77 * 479], 0);
        }
        // Even an injected unreachable pending field must not alter 274.
        for message in ["Message", ""] {
            c.tut_com_message = Some(message.into());
            c.redraw_chat = true;
            r.game_draw(&mut c);
            assert_chat(&r, &underlying);
            r.area_chat.as_mut().unwrap().pixels.fill(0xeeeeee);
            r.game_draw(&mut c);
            assert!(r
                .area_chat
                .as_ref()
                .unwrap()
                .pixels
                .iter()
                .all(|&p| p == 0xeeeeee));
            assert!(!c.redraw_chat);
        }
    }
}
