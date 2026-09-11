use client::client::{Client, ClientConfig};
use client::dash3d::ClientNpc;
use client::render::npc_overlay_box;

fn projected_client() -> Client {
    let mut client = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    client.ingame = true;
    client.scene_state = 2;
    client.minusedlevel = 0;
    client.cam_x = 6400;
    client.cam_y = 0;
    client.cam_z = 5000;
    client.cam_pitch = 0;
    client.cam_yaw = 0;

    let mut npc = ClientNpc::at(49, 46);
    npc.r#type = Some(0);
    npc.entity.x = 6400;
    npc.entity.z = 6000;
    npc.entity.size = 1;
    npc.entity.height = 100;
    client.npc[7] = Some(Box::new(npc));
    client.npc_ids[0] = 7;
    client.npc_count = 1;
    client
}

#[test]
fn npc_overlay_box_matches_overlay_canvas_projection_and_winding() {
    let client = projected_client();

    assert_eq!(
        npc_overlay_box(&client, 7),
        Some([
            (225, 171),
            (295, 171),
            (290, 171),
            (230, 171),
            (225, 117),
            (295, 117),
            (290, 123),
            (230, 123),
        ])
    );
}

#[test]
fn npc_overlay_box_is_unavailable_outside_a_ready_game_scene() {
    let mut client = projected_client();
    client.scene_state = 1;
    assert_eq!(npc_overlay_box(&client, 7), None);

    client.scene_state = 2;
    client.ingame = false;
    assert_eq!(npc_overlay_box(&client, 7), None);
}

#[test]
fn npc_overlay_box_rejects_unresolved_size_or_height() {
    let mut client = projected_client();
    client.npc[7].as_mut().unwrap().entity.height = 0;
    assert_eq!(npc_overlay_box(&client, 7), None);

    client.npc[7].as_mut().unwrap().entity.height = 100;
    client.npc[7].as_mut().unwrap().entity.size = 0;
    assert_eq!(npc_overlay_box(&client, 7), None);

    client.npc[7].as_mut().unwrap().entity.size = 1;
    client.npc[7].as_mut().unwrap().r#type = None;
    assert_eq!(npc_overlay_box(&client, 7), None);
}

#[test]
fn npc_overlay_box_rejects_points_behind_the_camera() {
    let mut client = projected_client();
    client.npc[7].as_mut().unwrap().entity.z = 4900;
    assert_eq!(npc_overlay_box(&client, 7), None);
}
