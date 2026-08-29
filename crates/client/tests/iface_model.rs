// TYPE_MODEL: `NpcType::get_head`, `ObjType::get_model_unlit`, and the
// `draw_interface` TYPE_MODEL arm (`IfType::get_temp_model` + `objRender`).
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and `Model::load` of a missing id is None.
use client::config::npc_type::NpcType;
use client::render::Renderer;
use client::config::obj_type::ObjType;
use client::dash3d::Model;

#[test]
fn npc_get_head_none_without_head_ids() {
let _r = Renderer::new(false);
    let npc = NpcType::default();
    assert!(npc.get_head().is_none());
}

#[test]
fn obj_get_model_unlit_none_without_model() {
let _r = Renderer::new(false);
    let obj = ObjType::default();
    let cache = client::config::Cache::default();
    assert!(obj.get_model_unlit(&cache, 50).is_none());
}

#[test]
fn draw_interface_type_model_missing_does_not_panic() {
let mut r = Renderer::new(false);
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
    c.ifaces.resize(3, None);
    c.ifaces[1] = Some(Box::new(layer));
    c.ifaces[2] = Some(Box::new(model));
    let mut pixels = vec![0i32; 50 * 50];
    let mut surface = client::graphics::Pix2D::with_pixels(&mut pixels, 50, 50);
    r.pix3d.set_clipping(50, 50);
    r.draw_interface(&mut c, 1, 0, 0, 0, &mut surface);
}

#[test]
fn npc_get_head_queues_every_missing_head_id() {
let _r = Renderer::new(false);
    use client::dash3d::model::ModelProvider;
    use std::sync::{Arc, Mutex};

    /// OnDemand hook counting every `request_model` call, so the test can
    /// assert each head id was queued even when the head is not ready.
    struct CountingProvider {
        requested: Arc<Mutex<Vec<i32>>>,
    }
    impl ModelProvider for CountingProvider {
        fn request_model(&mut self, id: i32) {
            self.requested.lock().unwrap().push(id);
        }
    }

    let requested = Arc::new(Mutex::new(Vec::new()));
    Model::init(7, Box::new(CountingProvider { requested: requested.clone() }));

    let npc = NpcType {
        head: Some(vec![5, 6]),
        ..NpcType::default()
    };
    assert!(npc.get_head().is_none(), "missing head models -> None");
    assert_eq!(
        *requested.lock().unwrap(),
        vec![5, 6],
        "both head ids must queue, not just the first"
    );
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

/// GPU `game_draw` with the mysterious-cube main modal (`macro_cube` 6554)
/// plus live `if_setobject` on the three spinning TYPE_MODEL children.
#[test]
fn gpu_draw_does_not_crash_on_mysterious_cube_modal() {
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&format!("{cache}/interface")).is_file() {
        eprintln!("no client cache; skip cube GPU repro");
        return;
    }

    struct NoopProvider;
    impl client::dash3d::model::ModelProvider for NoopProvider {
        fn request_model(&mut self, _id: i32) {}
    }
    client::dash3d::AnimFrame::init(40000);
    client::dash3d::Model::init(70000, Box::new(NoopProvider));
    let home = std::env::var("HOME").unwrap();
    client::unpack::load_snapshot(&cache, &format!("{home}/.274bot/unpack"))
        .expect("274 snapshot");

    client::render::Renderer::set_prefer_gpu(true);
    let mut r = client::render::Renderer::new(false);
    if r.backend_kind() != client::render::backend::BackendKind::Gpu {
        eprintln!("no adapter; skip cube GPU repro");
        client::render::Renderer::set_prefer_gpu(false);
        return;
    }

    let mut c = client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    r.game_draw(&mut c);
    assert!(
        c.ifaces.get(6554).and_then(|o| o.as_ref()).is_some(),
        "macro_cube interface 6554 must unpack from the 274 interface jag"
    );

    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.main_modal_id = 6554;
    for (com_id, obj_id) in [(6555, 3063), (6557, 3069), (6559, 3081)] {
        let (xan2d, yan2d, zoom2d) = if (obj_id as usize) < c.cache.objs.len() {
            let t = c.cache.obj(obj_id as usize);
            (t.xan2d, t.yan2d, t.zoom2d)
        } else {
            (0, 0, 0)
        };
        if let Some(com) = c.ifaces.get_mut(com_id).and_then(|o| o.as_mut()) {
            com.model1_type = 4;
            com.model1_id = obj_id;
            com.model_xan = xan2d;
            com.model_yan = yan2d;
            com.model_zoom = if 390 == 0 { 0 } else { (zoom2d * 100) / 390 };
        }
    }

    let frame = r.game_draw(&mut c);
    client::render::Renderer::set_prefer_gpu(false);
    let client::render::backend::FrameOutput::Texture(handle) = frame else {
        panic!("GPU cube frame must be a texture");
    };
    let pixels = handle.read_back();
    let mut painted = 0usize;
    for y in 130..160 {
        for x in 250..280 {
            if pixels[y * 765 + x] != 0 {
                painted += 1;
            }
        }
    }
    assert!(
        painted > 50,
        "mysterious cube TYPE_MODEL must composite into the GPU frame (got {painted} px)"
    );
}
