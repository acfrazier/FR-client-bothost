// TYPE_MODEL: `NpcType::get_head`, `ObjType::get_model_unlit`, and the
// `draw_interface` TYPE_MODEL arm (`IfType::get_temp_model` + `objRender`).
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and `Model::load` of a missing id is None.
use client::config::npc_type::NpcType;
use client::config::obj_type::ObjType;
use client::dash3d::Model;

#[test]
fn npc_get_head_none_without_head_ids() {
    let npc = NpcType::default();
    assert!(npc.get_head().is_none());
}

#[test]
fn obj_get_model_unlit_none_without_model() {
    let obj = ObjType::default();
    let cache = client::config::Cache::default();
    assert!(obj.get_model_unlit(&cache, 50).is_none());
}

#[test]
fn draw_interface_type_model_missing_does_not_panic() {
    let mut c = hud_client();
    let mut layer = client::config::if_type::IfType::default();
    layer.r#type = client::config::if_type::ComponentType::TYPE_LAYER;
    layer.width = 50;
    layer.height = 50;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut model = client::config::if_type::IfType::default();
    model.r#type = client::config::if_type::ComponentType::TYPE_MODEL;
    model.width = 50;
    model.height = 50;
    model.model1_type = 1;
    model.model1_id = 999999; // not loaded
    model.model_zoom = 800;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(model);
    let mut pixels = vec![0i32; 50 * 50];
    let mut surface = client::graphics::Pix2D::with_pixels(&mut pixels, 50, 50);
    c.pix3d.set_clipping(50, 50);
    c.draw_interface(1, 0, 0, 0, &mut surface);
}

fn hud_client() -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}
