// The `draw` CPU-save switch: `mainredraw` skips the whole frame render
// when `draw` is false, independent of the window (`client-play` sets it
// true after `WindowTarget::open`; headless bots on later 50-client hosts
// keep it false). The /tmp cache has no packs, so `Client::new` falls back
// to `Cache::default()` and never touches the network (see hud.rs).
use client::client::{Client, ClientConfig};
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
fn headless_new_starts_with_draw_off() {
    let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    assert!(!c.draw);
    let before = r.draw_area.pixels.clone();
    c.ingame = true;
    r.mainredraw(&mut c);
    assert_eq!(r.draw_area.pixels, before);
    // draw=false skips the frame render entirely: prepare_game never ran
    assert!(
        r.area_game.is_none(),
        "mainredraw must skip with draw=false"
    );
}

#[test]
fn set_draw_true_allows_game_draw_without_present() {
    let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    r.mainredraw(&mut c); // must not panic; prepare_game may run
    assert!(
        r.area_game.is_some(),
        "mainredraw with draw=true runs the game draw"
    );
}

/// The host hook must reproduce the ordinary renderer's persistent state,
/// including a delayed effect, a moving projectile target, UI and orbit.
fn animation_state_at_first_draw(hosted: bool) -> Vec<u64> {
    use client::client::{ClientNpc, ClientPlayer};
    use client::config::if_type::{ComponentType, IfType, IfTypeMut};
    use client::config::{Cache, SeqType, SpotType};
    use client::dash3d::{ClientProj, MapSpotAnim};
    use std::sync::Arc;

    let mut c = client();
    c.cache = Arc::new(Cache {
        seqs: vec![SeqType {
            num_frames: 3,
            frames: Some(vec![-1; 3]),
            delay: Some(vec![4, 6, 5]),
            loops: 3,
            ..Default::default()
        }],
        spots: vec![SpotType {
            seq: Some(0),
            ..Default::default()
        }],
        ..Default::default()
    });
    c.error_loading = false;
    c.ingame = true;
    c.scene_state = 2;
    c.minimap_level = c.minusedlevel;
    c.local_player = Some(ClientPlayer::at(50, 50));
    c.npc[7] = Some(Box::new(ClientNpc::at(52, 50)));
    c.spotanims
        .push(MapSpotAnim::new(0, 0, 6464, 6464, 0, 0, 10));
    // Wrong-level and already-expired effects must be pruned too.
    c.spotanims
        .push(MapSpotAnim::new(0, 1, 6464, 6464, 0, 0, 0));
    let mut projectile = ClientProj::new(0, 0, 6464, 0, 6464, 10, 200, 15, 0, 8, 0);
    projectile.bind_seq(&c.cache);
    projectile.set_target(6720.0, 0.0, 6464.0, 10);
    c.projectiles.push(projectile);
    c.projectiles
        .push(ClientProj::new(0, 0, 6464, 0, 6464, 0, 5, 0, 0, 0, 0));
    c.set_iface(
        1,
        IfType {
            r#type: ComponentType::TYPE_LAYER,
            children: Some(vec![2]),
            child_x: Some(vec![0]),
            child_y: Some(vec![0]),
            ..Default::default()
        },
    );
    c.set_iface(
        2,
        IfType {
            r#type: ComponentType::TYPE_MODEL,
            ..Default::default()
        },
    );
    c.set_iface_mut(
        2,
        IfTypeMut {
            model_anim: 0,
            ..Default::default()
        },
    );
    c.side_modal_id = 1;
    let mut renderer = Renderer::new(false);
    for cycle in 1..=120 {
        c.loop_cycle = cycle;
        c.world_update_num = 1;
        c.local_player.as_mut().unwrap().x += 2;
        c.npc[7].as_mut().unwrap().x += 1;
        if hosted {
            c.advance_hosted_animations(false);
        }
        if !hosted || cycle == 120 {
            c.set_draw(true);
            renderer.mainredraw(&mut c);
        }
    }
    assert!(c.spotanims.head().is_none(), "old map smoke must expire");
    let mut state = vec![
        c.orbit_camera_x as u64,
        c.orbit_camera_z as u64,
        c.camera_pitch_clamp as u64,
        c.if_(2).unwrap().anim_frame as u64,
        c.if_(2).unwrap().anim_cycle as u64,
    ];
    c.projectiles.for_each(|projectile| {
        state.extend([
            projectile.x.to_bits(),
            projectile.y.to_bits(),
            projectile.z.to_bits(),
            projectile.velocity_y.to_bits(),
            projectile.anim_frame as u64,
            projectile.anim_cycle as u64,
        ]);
    });
    assert_eq!(state.len(), 11, "only the active projectile survives");
    state
}

#[test]
fn hosted_undrawn_animations_match_drawn_state_without_double_aging() {
    assert_eq!(
        animation_state_at_first_draw(true),
        animation_state_at_first_draw(false)
    );
}
