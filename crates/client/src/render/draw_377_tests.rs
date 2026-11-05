use std::sync::Arc;

use super::Renderer;
use crate::client::client::{Client, ClientConfig};
use crate::config::{Cache, LocType, NpcType};
use crate::dash3d::{ClientNpc, ClientPlayer};
use crate::graphics::{Pix32, Pix8, PixMap};
use crate::io::ClientRevision;
use crate::render::media::{Media, Media377};

fn client(revision: ClientRevision, cache: Cache) -> Client {
    let mut client = Client::from_shared_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: std::env::temp_dir()
                .join(format!("revision-377-sprites-{}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            members: true,
            lowmem: true,
        },
        Arc::new(cache),
        Arc::new(Vec::new()),
        Arc::new(Vec::new()),
        revision,
    );
    client.ingame = true;
    client.cam_x = 0;
    client.cam_y = 0;
    client.cam_z = 0;
    client.cam_pitch = 0;
    client.cam_yaw = 0;
    client
}

fn renderer() -> Renderer {
    let mut renderer = Renderer::new(false);
    renderer.media = Arc::new(Media::empty());
    renderer.area_game = Some(PixMap::new(512, 334));
    renderer.area_map = Some(PixMap::new(172, 156));
    renderer.pix3d.set_clipping(512, 334);
    renderer.pix3d.origin_x = 256;
    renderer.pix3d.origin_y = 167;
    renderer
}

fn sprite(rgb: i32) -> Pix32 {
    let mut sprite = Pix32::new(1, 1);
    sprite.data[0] = rgb;
    sprite
}

fn pixel(pixels: &[i32], width: usize, x: i32, y: i32) -> i32 {
    pixels[y as usize * width + x as usize]
}

fn player(name: &str, x: i32, z: i32, team: i32) -> ClientPlayer {
    ClientPlayer {
        entity: crate::dash3d::client_entity::ClientEntity {
            x,
            z,
            ..Default::default()
        },
        ready: true,
        name: Some(name.to_string()),
        team_377: team,
        ..ClientPlayer::default()
    }
}

fn dot_media() -> Arc<Media> {
    let mut media = Media::empty();
    media.mapdots3 = Some(sprite(0x222222));
    media.mapdots4 = Some(sprite(0x333333));
    let mut revision_377 = Media377::empty();
    revision_377.mapdots5 = Some(sprite(0x444444));
    media.sprites_377 = Some(revision_377);
    Arc::new(media)
}

fn player_dot_client(revision: ClientRevision) -> Client {
    let mut client = client(revision, Cache::default());
    client.local_player = Some(ClientPlayer {
        entity: crate::dash3d::client_entity::ClientEntity {
            x: 3200,
            z: 3200,
            ..Default::default()
        },
        team_377: 42,
        ..ClientPlayer::default()
    });
    client.player_count = 3;
    client.player_ids[..3].copy_from_slice(&[0, 1, 2]);
    client.players[0] = Some(Box::new(player("friend", 3360, 3200, 42)));
    client.players[1] = Some(Box::new(player("teammate", 3520, 3200, 42)));
    client.players[2] = Some(Box::new(player("ordinary", 3680, 3200, 0)));
    client.friend_count = 1;
    client.friend_username[0] = "Friend".to_string();
    client.friend_node_id[0] = 301;
    client.macro_minimap_angle = 0;
    client.macro_minimap_zoom = 0;
    client.orbit_camera_yaw = 0;
    client
}

#[test]
fn minimap_377_uses_friend_then_team_then_ordinary_dots() {
    let media = dot_media();
    let mut renderer_377 = renderer();
    renderer_377.media = media.clone();
    let mut client_377 = player_dot_client(ClientRevision::R377);
    renderer_377.minimap_draw(&mut client_377);
    let pixels_377 = &renderer_377.area_map.as_ref().unwrap().pixels;
    assert_eq!(pixel(pixels_377, 172, 103, 79), 0x333333);
    assert_eq!(pixel(pixels_377, 172, 108, 79), 0x444444);
    assert_eq!(pixel(pixels_377, 172, 113, 79), 0x222222);

    for revision in [ClientRevision::R274, ClientRevision::R289] {
        let mut renderer_legacy = renderer();
        renderer_legacy.media = media.clone();
        let mut client_legacy = player_dot_client(revision);
        renderer_legacy.minimap_draw(&mut client_legacy);
        let pixels_legacy = &renderer_legacy.area_map.as_ref().unwrap().pixels;
        assert_eq!(pixel(pixels_legacy, 172, 103, 79), 0x222222);
        assert_eq!(pixel(pixels_legacy, 172, 108, 79), 0x222222);
        assert_eq!(pixel(pixels_legacy, 172, 113, 79), 0x222222);
    }
}

#[test]
fn high_mapscene_and_mapfunction_indices_reach_minimap_rendering() {
    let cache = Cache {
        locs: vec![LocType {
            mapscene: 71,
            mapfunction: 69,
            ..LocType::default()
        }],
        ..Cache::default()
    };
    let mut client = client(ClientRevision::R377, cache);
    client.minusedlevel = 0;
    client.minimap_state = 0;
    client.local_player = Some(ClientPlayer {
        entity: crate::dash3d::client_entity::ClientEntity {
            x: 45 * 128 + 64,
            z: 45 * 128 + 64,
            ..Default::default()
        },
        ..ClientPlayer::default()
    });
    client.world.set_ground_decor(0, 50, 50, 0, 1, 0, 0, 0, 0, 0);

    let mut revision_377 = Media377::empty();
    let mut scene = Pix8::new(1, 1, vec![0, 0x123456]);
    scene.data[0] = 1;
    revision_377.mapscene[71] = Some(scene);
    revision_377.mapfunction[69] = Some(sprite(0x345678));
    let mut media = Media::empty();
    media.sprites_377 = Some(revision_377);

    let mut renderer = renderer();
    renderer.media = Arc::new(media);
    renderer.minimap = Some(Pix32::new(512, 512));
    renderer.minimap_build_buffer(&mut client, 0);

    let mapscene_color = renderer
        .media
        .sprites_377
        .as_ref()
        .unwrap()
        .mapscene[71]
        .as_ref()
        .unwrap()
        .bpal[1];
    let minimap = &renderer.minimap.as_ref().unwrap().data;
    assert_eq!(minimap[261 * 512 + 249], mapscene_color);
    assert_eq!(renderer.active_map_function_count, 1);
    assert_eq!(
        renderer.active_map_functions[0].as_ref().unwrap().data,
        [0x345678]
    );

    let anchor_x = renderer.active_map_function_x[0];
    let anchor_z = renderer.active_map_function_z[0];
    let player_x = client.local_player.as_ref().unwrap().x / 32;
    let player_z = client.local_player.as_ref().unwrap().z / 32;
    let icon = renderer.active_map_functions[0].as_ref().unwrap();
    let dot_x = anchor_x * 4 + 2 - player_x;
    let dot_y = anchor_z * 4 + 2 - player_z;
    let screen_x = dot_x + 94 - icon.owi / 2 + 4;
    let screen_y = 83 - dot_y - icon.ohi / 2 - 4;

    renderer.minimap = None;
    renderer.minimap_draw(&mut client);
    let area_map = &renderer.area_map.as_ref().unwrap().pixels;
    assert_eq!(pixel(area_map, 172, screen_x, screen_y), 0x345678);
}

#[test]
fn player_pk_prayer_and_hint_stack_use_distinct_377_tables() {
    let cache = Cache {
        npcs: vec![NpcType {
            headicon: 8,
            ..NpcType::default()
        }],
        ..Cache::default()
    };
    let mut client = client(ClientRevision::R377, cache);
    client.player_count = 1;
    client.player_ids[0] = 0;
    let mut player = player("icon stack", 384, 1280, 0);
    player.entity.height = 100;
    player.pk_icon_377 = 2;
    player.prayer_icon_377 = 3;
    player.headicons = 1;
    client.players[0] = Some(Box::new(player));
    client.hint_type = 10;
    client.hint_player = 0;
    client.npc_count = 1;
    client.npc_ids[0] = 0;
    let npc = ClientNpc {
        entity: crate::dash3d::client_entity::ClientEntity {
            x: 600,
            z: 1280,
            height: 100,
            ..Default::default()
        },
        r#type: Some(0),
        ..ClientNpc::default()
    };
    client.npc[0] = Some(Box::new(npc));

    let mut media = Media::empty();
    media.headicons[0] = Some(sprite(0x00AA00));
    media.headicons_hint[1] = Some(sprite(0xFFFF00));
    let mut revision_377 = Media377::empty();
    revision_377.headicons_pk[2] = Some(sprite(0xFF0000));
    revision_377.headicons_prayer[3] = Some(sprite(0x0000FF));
    revision_377.headicons_prayer[8] = Some(sprite(0xAA00AA));
    media.sprites_377 = Some(revision_377);

    let mut renderer = renderer();
    renderer.media = Arc::new(media);
    let player = client.players[0].as_ref().unwrap();
    let (player_x, player_y) = renderer.project_overlay(
        &client,
        player.x,
        player.z,
        player.height + 15,
    );
    let npc = client.npc[0].as_ref().unwrap();
    let (npc_x, npc_y) = renderer.project_overlay(&client, npc.x, npc.z, npc.height + 15);

    renderer.entity_overlays(&mut client);

    let pixels = &renderer.area_game.as_ref().unwrap().pixels;
    assert_eq!(pixel(pixels, 512, player_x - 12, player_y - 30), 0xFF0000);
    assert_eq!(pixel(pixels, 512, player_x - 12, player_y - 55), 0x0000FF);
    assert_eq!(pixel(pixels, 512, player_x - 12, player_y - 80), 0xFFFF00);
    assert_eq!(pixel(pixels, 512, npc_x - 12, npc_y - 30), 0xAA00AA);
    assert!(!pixels.contains(&0x00AA00));
}

#[test]
fn multiway_overlay_uses_java_377_sprite_and_preserves_legacy_rendering() {
    let mut media = Media::empty();
    media.headicons[1] = Some(sprite(0x00AA00));
    let mut revision_377 = Media377::empty();
    revision_377.overlay_multiway = Some(sprite(0xFF0000));
    media.sprites_377 = Some(revision_377);
    let media = Arc::new(media);

    for (revision, expected) in [
        (ClientRevision::R377, 0xFF0000),
        (ClientRevision::R289, 0x00AA00),
        (ClientRevision::R274, 0),
    ] {
        let mut client = client(revision, Cache::default());
        client.in_multizone = 1;
        let mut renderer = renderer();
        renderer.media = media.clone();
        renderer.other_overlays(&mut client);
        assert_eq!(
            renderer.area_game.as_ref().unwrap().pixels[296 * 512 + 472],
            expected
        );
    }
}
