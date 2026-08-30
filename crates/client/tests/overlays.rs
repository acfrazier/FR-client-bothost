// Task 5: `getOverlayPos` + `entityOverlays` — the prayer headicons, chat
// bubbles, health bars and hitmarks drawn into `area_game`. The projection
// tests need no cache; the sprite/draw tests need the real `media`/`title`
// packs and skip when they are absent (see hud.rs).
use std::sync::Arc;

use client::client::{Client, ClientConfig, ClientPlayer};
use client::render::Renderer;
use client::config::IdkType;
use client::dash3d::Model;
use client::graphics::{Colour, Pix32};
use std::collections::HashMap;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// A client whose `media` sprites and fonts are loaded (`prepare_game`).
fn overlay_client(cache: &str, r: &mut Renderer) -> Client {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache.into(),
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    r.game_draw(&mut c);
    c
}

/// The most common non-transparent colour of a Pix32 sprite.
fn sprite_fill_colour(sprite: &Pix32) -> i32 {
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for &rgb in &sprite.data {
        if rgb != 0 {
            *counts.entry(rgb).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map(|(c, _)| c)
        .unwrap_or(0)
}

/// Point the camera along +z from the scene origin (pitch/yaw 0) so a scene
/// coord with `z' >= 50` projects inside `area_game`'s 512×334 bounds.
fn look_down_z(c: &mut Client) {
    c.cam_x = 0;
    c.cam_y = 0;
    c.cam_z = 0;
    c.cam_pitch = 0;
    c.cam_yaw = 0;
}

#[test]
fn get_overlay_pos_out_of_range_is_neg1() {
let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    r.get_overlay_pos(&mut c, 0, 64, 0); // x < 128
    assert_eq!((r.project_x, r.project_y), (-1, -1));
    r.get_overlay_pos(&mut c, 13057, 64, 0); // x > 13056
    assert_eq!((r.project_x, r.project_y), (-1, -1));
    r.get_overlay_pos(&mut c, 64, 0, 0); // z < 128
    assert_eq!((r.project_x, r.project_y), (-1, -1));
    r.get_overlay_pos(&mut c, 64, 13057, 0); // z > 13056
    assert_eq!((r.project_x, r.project_y), (-1, -1));
}

#[test]
fn get_overlay_pos_projects_in_front_of_camera() {
let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    r.pix3d.origin_x = 256;
    r.pix3d.origin_y = 167;
    look_down_z(&mut c);
    // dz = 1280 - 0 >= 50; pitch/yaw 0 reduces the rotate to dx/dz.
    r.get_overlay_pos(&mut c, 640, 1280, 0);
    assert_eq!(r.project_x, 256 + (640 * 512) / 1280);
    assert_eq!(r.project_y, 167);
    // A point at the same height as the camera projects onto the y origin.
    r.get_overlay_pos(&mut c, 384, 1280, 0);
    assert!(r.project_x > -1 && r.project_y > -1);
    // Behind the camera (z' < 50) is -1.
    r.get_overlay_pos(&mut c, 384, 40, 0);
    assert_eq!((r.project_x, r.project_y), (-1, -1));
}

#[test]
fn prepare_game_depacks_headicons() {
let mut r = Renderer::new(false);
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let _c = overlay_client(&cache, &mut r);
    assert!(r.media.headicons[3].is_some(), "protect-melee headicon must depack");
    assert!(r.media.headicons[4].is_some(), "protect-missiles headicon must depack");
    assert!(r.media.headicons[5].is_some(), "protect-magic headicon must depack");
    assert!(r.media.hitmarks.iter().take(4).any(|s| s.is_some()));
}

#[test]
fn entity_overlays_plots_prayer_headicon() {
let mut r = Renderer::new(false);
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = overlay_client(&cache, &mut r);
    let melee = sprite_fill_colour(r.media.headicons[3].as_ref().expect("headicons[3] depacked"));
    assert_ne!(melee, 0, "the protect-melee headicon must have drawn pixels");
    look_down_z(&mut c);
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.entity.x = 384;
    player.entity.z = 1280;
    player.entity.height = 100;
    c.local_player = Some(player);
    r.entity_overlays(&mut c);
    let control = r.area_game.as_ref().unwrap().pixels.clone();
    c.local_player.as_mut().unwrap().headicons = 1 << 3; // protect-melee bit
    r.entity_overlays(&mut c);
    let rendered = r.area_game.as_ref().unwrap().pixels.clone();
    let n_control = control.iter().filter(|&&p| p == melee).count();
    let n_rendered = rendered.iter().filter(|&&p| p == melee).count();
    assert!(
        n_rendered > n_control,
        "the protect-melee headicon ({melee:#06x}) must plot into area_game \
         (control {n_control}, drawn {n_rendered})"
    );
}

#[test]
fn entity_overlays_collects_chat_bubble() {
let mut r = Renderer::new(false);
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = overlay_client(&cache, &mut r);
    look_down_z(&mut c);
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.entity.x = 384;
    player.entity.z = 1280;
    player.entity.height = 100;
    player.entity.chat_message = Some("hi".into());
    c.local_player = Some(player);
    r.entity_overlays(&mut c);
    assert!(r.chat_count >= 1, "the chat bubble must be collected");
    assert_eq!(r.chats[0], "hi");
    // The bubble draws centred black-then-yellow text in area_game.
    let game = r.area_game.as_ref().unwrap();
    assert!(
        game.pixels.contains(&Colour::YELLOW),
        "the bubble text must draw yellow pixels into area_game"
    );
}

/// A hand-crafted model 100 above the origin (`min_y` = 100): 3 points, one
/// face at y = -100 (the engine's y axis points down, so `min_y` = 100).
/// The 18-byte trailer claims 3 points, 1 face, y-delta data length 4.
const LIFTED_MODEL: &[u8] = &[
    7, 7, 7, // vertex order: x+y+z for each of the 3 vertices
    1, // face index order: a,b,c are deltas
    0x40, 0x41, 0x41, // face index deltas: a=0, b=1, c=2
    0x11, 0x22, // face colour
    0x40, 0x72, 0x0e, // vertexX deltas: 0, 50, -50
    0xbf, 0x9c, 0x40, 0x40, // vertexY deltas: -100, -100, -100
    0x40, 0x40, 0x72, // vertexZ deltas: 0, 0, 50
    0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 4, 0, 3, 0, 3, // trailer
];

/// Java 8870 `entityOverlays` reads `entity.height`, which `getTempModel`
/// stamps as the model `min_y` on the *same* object (Java 8870 vs
/// ClientPlayer.getTempModel 166-174). Here the scene sprite is a clone, so
/// the live local player's height stays 0 unless `add_players` stamps it.
/// After a `gameDrawMain` pass the live player's height must be the model
/// `min_y` (100), which is also where the headicons project from
/// (`height + 15`).
#[test]
fn add_players_stamps_model_min_y_on_live_player() {
let _r = Renderer::new(false);
    Model::unpack(4096, Some(LIFTED_MODEL));
    let mut r = Renderer::new(false);
    let mut c = client();
    while c.cache.idks.is_empty() {
        Arc::get_mut(&mut c.cache).unwrap().idks.push(IdkType::default());
    }
    Arc::get_mut(&mut c.cache).unwrap().idks[0].model = Some(vec![4096]);
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.entity.x = 384;
    player.entity.z = 384;
    player.appearance[0] = 256; // head = idk 0, whose model is LIFTED_MODEL
    player.colour[0] = 1; // distinct base_id so no stale model-cache hit
    c.local_player = Some(player);
    c.ingame = true;
    c.scene_state = 2;
    r.game_draw(&mut c);
    let p = c.local_player.as_ref().unwrap();
    assert_eq!(
        p.entity.height, 100,
        "add_players must stamp the live player height with the model min_y"
    );
}
