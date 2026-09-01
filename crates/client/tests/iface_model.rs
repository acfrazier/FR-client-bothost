// TYPE_MODEL: `NpcType::get_head`, `ObjType::get_model_unlit`, and the
// `draw_interface` TYPE_MODEL arm (`IfType::get_temp_model` + `objRender`).
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and `Model::load` of a missing id is None.
use client::config::npc_type::NpcType;
use client::config::obj_type::ObjType;
use client::dash3d::Model;
use client::render::Renderer;

#[test]
fn npc_get_head_none_without_head_ids() {
    let _r = Renderer::new(false);
    let npc = NpcType::default();
    assert!(npc.get_head().is_none());
}

#[test]
fn obj_get_model_unlit_none_without_model() {
    Model::reset_for_tests();
    let obj = ObjType::default();
    let cache = client::config::Cache::default();
    assert!(obj.get_model_unlit(&cache, 50).is_none());
}

#[test]
fn draw_interface_type_model_missing_does_not_panic() {
    let mut r = Renderer::new(false);
    let mut c = hud_client();
    let layer = client::config::if_type::IfType {
        r#type: client::config::if_type::ComponentType::TYPE_LAYER,
        width: 50,
        height: 50,
        children: Some(vec![2]),
        child_x: Some(vec![0]),
        child_y: Some(vec![0]),
        ..Default::default()
    };
    let model = client::config::if_type::IfType {
        r#type: client::config::if_type::ComponentType::TYPE_MODEL,
        width: 50,
        height: 50,
        ..Default::default()
    };
    let model_mut = client::config::if_type::IfTypeMut {
        model1_type: 1,
        model1_id: 999999, // not loaded
        model_zoom: 800,
        ..Default::default()
    };
    c.set_iface(1, layer);
    c.set_iface(2, model);
    c.set_iface_mut(2, model_mut);
    let mut pixels = vec![0i32; 50 * 50];
    let mut surface = client::graphics::Pix2D::with_pixels(&mut pixels, 50, 50);
    r.pix3d.set_clipping(50, 50);
    r.draw_interface(&mut c, 1, 0, 0, 0, &mut surface);
}

#[test]
fn npc_get_head_queues_every_missing_head_id() {
    Model::reset_for_tests();
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
    Model::init(
        7,
        Box::new(CountingProvider {
            requested: requested.clone(),
        }),
    );

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
    client::unpack::load_snapshot(&cache, &format!("{home}/.274bot/unpack")).expect("274 snapshot");

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
        c.if_(6554).is_some(),
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
        if let Some(com) = c.iface_mut(com_id) {
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

/// ship_journey / glidermap are main-modals drawn into `area_game`. On GPU
/// the scene window is a 3D hole unless overlay coverage is marked: a
/// TYPE_RECT / TYPE_TEXT child must stay opaque over the scene, not only
/// TYPE_MODEL (the little boats).
#[test]
fn gpu_main_modal_rect_is_opaque_over_the_scene() {
    client::render::Renderer::set_prefer_gpu(true);
    let mut r = client::render::Renderer::new(false);
    if r.backend_kind() != client::render::backend::BackendKind::Gpu {
        eprintln!("no adapter; skip GPU modal coverage");
        client::render::Renderer::set_prefer_gpu(false);
        return;
    }
    let mut c = hud_client();
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    r.game_draw(&mut c);

    const ROOT: i32 = 90;
    const RECT: i32 = 91;
    const SEA: i32 = 0x0033_6699;
    c.set_iface(
        ROOT as usize,
        client::config::if_type::IfType {
            id: ROOT,
            r#type: client::config::if_type::ComponentType::TYPE_LAYER,
            width: 512,
            height: 334,
            children: Some(vec![RECT]),
            child_x: Some(vec![0]),
            child_y: Some(vec![0]),
            fill: true,
            ..Default::default()
        },
    );
    c.set_iface(
        RECT as usize,
        client::config::if_type::IfType {
            id: RECT,
            r#type: client::config::if_type::ComponentType::TYPE_RECT,
            width: 512,
            height: 334,
            fill: true,
            ..Default::default()
        },
    );
    c.set_iface_mut(
        RECT as usize,
        client::config::if_type::IfTypeMut {
            colour: SEA,
            ..Default::default()
        },
    );
    c.main_modal_id = ROOT;

    let frame = r.game_draw(&mut c);
    client::render::Renderer::set_prefer_gpu(false);
    let client::render::backend::FrameOutput::Texture(handle) = frame else {
        panic!("GPU modal frame must be a texture");
    };
    let pixels = handle.read_back();
    let rgb = pixels[24 * 765 + 24] & 0x00ff_ffff;
    assert_eq!(
        rgb, SEA,
        "main-modal TYPE_RECT must cover the GPU scene hole, got {rgb:#08x}"
    );
}

/// The packed 274 `ship_journey` / `glidermap` roots must be full-scene
/// layers (otherwise TYPE_TEXT/RECT clip away and only TYPE_MODEL boats
/// survive — the live GPU miss).
#[test]
fn travel_modals_are_full_scene_layers() {
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&format!("{cache}/interface")).is_file() {
        eprintln!("no client cache; skip travel modal sizes");
        return;
    }
    let mut r = client::render::Renderer::new(false);
    let mut c = client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    r.game_draw(&mut c);
    for (id, name) in [(3281, "ship_journey"), (802, "glidermap")] {
        let Some(root) = c.if_(id) else {
            panic!("{name} ({id}) must unpack from the 274 interface jag");
        };
        assert_eq!(
            root.r#type,
            client::config::if_type::ComponentType::TYPE_LAYER,
            "{name} is a layer"
        );
        assert!(
            root.width >= 512 && root.height >= 334,
            "{name} must cover area_game (got {}x{})",
            root.width,
            root.height
        );
        let n = root.children.as_ref().map(|ch| ch.len()).unwrap_or(0);
        assert!(n > 1, "{name} has dest/label children, got {n}");
    }
}

/// Live 274 `ship_journey` (3281) on GPU: the yellow title
/// (`You Journey on the ship....`) must composite, not only TYPE_MODEL
/// boats. Skips without cache/adapter.
#[test]
fn gpu_ship_journey_paints_the_title_over_the_scene() {
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&format!("{cache}/interface")).is_file()
        || !std::path::Path::new(&format!("{cache}/title")).is_file()
    {
        eprintln!("no client cache; skip ship_journey GPU title");
        return;
    }
    client::render::Renderer::set_prefer_gpu(true);
    let mut r = client::render::Renderer::new(false);
    if r.backend_kind() != client::render::backend::BackendKind::Gpu {
        eprintln!("no adapter; skip ship_journey GPU title");
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
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    r.game_draw(&mut c);
    c.main_modal_id = 3281;
    let frame = r.game_draw(&mut c);
    client::render::Renderer::set_prefer_gpu(false);
    let client::render::backend::FrameOutput::Texture(handle) = frame else {
        panic!("GPU ship_journey frame must be a texture");
    };
    let pixels = handle.read_back();
    let mut any = 0usize;
    for y in 4..338 {
        for x in 4..516 {
            if pixels[y * 765 + x] & 0x00ff_ffff != 0 {
                any += 1;
            }
        }
    }
    // area_game is 512×334 = 171008. A couple of TYPE_MODEL boats is a
    // few thousand px; the chart + labels fill the scene window.
    assert!(
        any > 50_000,
        "ship_journey chart must fill the GPU scene, not only boats (got {any} overlay px)"
    );
}

/// GPU freeze (`scene_state==1`) must still composite `ship_journey`, not
/// drop to the last 3D texture (dock boats, no chart).
#[test]
fn gpu_ship_journey_stays_over_a_frozen_scene() {
    let cache = client::cache_dir().display().to_string();
    if !std::path::Path::new(&format!("{cache}/interface")).is_file()
        || !std::path::Path::new(&format!("{cache}/title")).is_file()
    {
        eprintln!("no client cache; skip ship_journey freeze");
        return;
    }
    client::render::Renderer::set_prefer_gpu(true);
    let mut r = client::render::Renderer::new(false);
    if r.backend_kind() != client::render::backend::BackendKind::Gpu {
        eprintln!("no adapter; skip ship_journey freeze");
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
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    r.game_draw(&mut c);
    c.main_modal_id = 3281;
    c.scene_state = 1;
    let frame = r.game_draw(&mut c);
    client::render::Renderer::set_prefer_gpu(false);
    let client::render::backend::FrameOutput::Texture(handle) = frame else {
        panic!("GPU freeze frame must be a texture");
    };
    let pixels = handle.read_back();
    let mut any = 0usize;
    for y in 4..338 {
        for x in 4..516 {
            if pixels[y * 765 + x] & 0x00ff_ffff != 0 {
                any += 1;
            }
        }
    }
    assert!(
        any > 50_000,
        "frozen scene must keep the ship_journey chart (got {any} overlay px)"
    );
}
