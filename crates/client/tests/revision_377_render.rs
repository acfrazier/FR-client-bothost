//! Revision 377 entity-hint sprites differ from the legacy media indices.
use std::sync::{Arc, Mutex};

use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision, MiniMenuAction};
use client::config::if_type::ComponentType;
use client::config::runtime_377::{
    CacheFields377, InterfaceFields377, LocFields377, MorphTable377,
};
use client::config::{
    Cache, CacheRevision377, IfType, IfTypeMut, LocType, SeqType, VarBitType, VarpType,
};
use client::core::{ClientBuild, World};
use client::dash3d::model::ModelProvider;
use client::dash3d::{
    AnimFrame, ClientLocAnim, ClientNpc, CollisionMap, LocShape, Model, SceneModel,
};
use client::graphics::{Pix2D, Pix32, PixMap};
use client::io::Packet;
use client::render::{media::Media, RenderWorld, Renderer};

static MODEL_FIXTURE_LOCK: Mutex<()> = Mutex::new(());

const OLD_NPC_HINT: i32 = 0x00ff00;
const R377_NPC_HINT: i32 = 0xff0000;
const OLD_PLAYER_HINT: i32 = 0x0000ff;
const R377_PLAYER_HINT: i32 = 0xffff00;

// Client-Java 377 model 5201 and animation archive 399, embedded as cache
// bytes so this regression always exercises the real decoder without a
// local cache or engine. The animation fixture is frames 12334..=12338.
const R377_BANNER_MODEL_5201: &[u8] = &[
    0x03, 0x03, 0x01, 0x03, 0x01, 0x03, 0x05, 0x03, 0x03, 0x04, 0x03, 0x02, 0x03, 0x01, 0x04, 0x03,
    0x03, 0x03, 0x03, 0x03, 0x04, 0x07, 0x03, 0x03, 0x01, 0x02, 0x02, 0x02, 0x02, 0x02, 0x03, 0x02,
    0x02, 0x02, 0x02, 0x02, 0x03, 0x03, 0x02, 0x03, 0x03, 0x02, 0x02, 0x02, 0x03, 0x02, 0x03, 0x02,
    0x02, 0x03, 0x03, 0x03, 0x02, 0x02, 0x02, 0x03, 0x01, 0x02, 0x03, 0x02, 0x03, 0x02, 0x02, 0x03,
    0x03, 0x01, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x41, 0x41, 0x41,
    0x41, 0x41, 0x41, 0x3b, 0x46, 0x41, 0x41, 0x41, 0x41, 0x37, 0x41, 0x49, 0x38, 0x41, 0x48, 0x41,
    0x41, 0x37, 0x4a, 0x37, 0x4a, 0x41, 0x36, 0x41, 0x41, 0x49, 0x41, 0x39, 0x3e, 0x41, 0x44, 0x3f,
    0x46, 0x3c, 0x45, 0x3c, 0x45, 0x37, 0x45, 0x41, 0x39, 0x48, 0x3a, 0x3f, 0x49, 0x3f, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2b, 0xc0, 0x2b, 0xc0, 0x2b, 0xc0, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x2b, 0xc0, 0x2b, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x2b, 0xc0, 0xbd, 0x50, 0xc0, 0x80, 0xc0, 0x40, 0x00, 0xc4, 0x80, 0xc0,
    0x80, 0xba, 0x80, 0xc0, 0x80, 0xbf, 0x90, 0xc2, 0xb0, 0xc2, 0x00, 0xc0, 0x40, 0xc0, 0x80, 0xbb,
    0x00, 0xc0, 0x40, 0xc2, 0x00, 0xc2, 0xb0, 0xbf, 0x90, 0xbd, 0xc0, 0xc2, 0x00, 0x08, 0xc0, 0x78,
    0xbf, 0xb0, 0x18, 0xc0, 0x78, 0x60, 0xc0, 0x80, 0x18, 0xbf, 0x88, 0xbf, 0x88, 0x68, 0xc0, 0x50,
    0xc0, 0xa0, 0xbf, 0x80, 0xbf, 0x90, 0xc0, 0xc8, 0xbf, 0x88, 0x50, 0x30, 0x50, 0x30, 0x50, 0x00,
    0x18, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x27, 0x00, 0x1d, 0x00, 0x05, 0x00,
    0x32,
];

const R377_BANNER_ANIMS_399: &[u8] = &[
    0x00, 0x05, 0x30, 0x2e, 0x02, 0x30, 0x2f, 0x02, 0x30, 0x30, 0x02, 0x30, 0x31, 0x02, 0x30, 0x32,
    0x02, 0x00, 0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x07, 0x00, 0x07, 0xc0, 0x7d, 0xc0, 0x7d, 0xc0,
    0x7d, 0xc0, 0x7a, 0xc0, 0x7a, 0xc0, 0x7a, 0xc0, 0x77, 0xc0, 0x77, 0xc0, 0x77, 0xc0, 0x74, 0xc0,
    0x74, 0xc0, 0x74, 0x06, 0x06, 0x06, 0x06, 0x06, 0x02, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x0f,
    0x00, 0x0a, 0x00, 0x18, 0x00, 0x05,
];

const JAVA_BANNER_FRAME_0: [[i32; 3]; 24] = [
    [-688, -56, 0],
    [-560, 64, 0],
    [-496, 64, 0],
    [-560, -16, 0],
    [592, -16, 0],
    [720, -56, 0],
    [-688, -56, 16],
    [-560, 64, 16],
    [-672, 96, 16],
    [-672, 96, 0],
    [16, 224, 0],
    [16, 184, 0],
    [528, 64, 0],
    [592, 64, 0],
    [592, 64, 16],
    [720, -56, 16],
    [-560, -16, 16],
    [-496, 64, 16],
    [16, 224, 16],
    [704, 96, 16],
    [704, 96, 0],
    [592, -16, 16],
    [16, 184, 16],
    [528, 64, 16],
];

const JAVA_BANNER_FRAME_1: [[i32; 3]; 24] = [
    [-671, -54, 0],
    [-546, 62, 0],
    [-484, 62, 0],
    [-546, -15, 0],
    [578, -15, 0],
    [703, -54, 0],
    [-671, -54, 15],
    [-546, 62, 15],
    [-656, 93, 15],
    [-656, 93, 0],
    [15, 218, 0],
    [15, 179, 0],
    [515, 62, 0],
    [578, 62, 0],
    [578, 62, 15],
    [703, -54, 15],
    [-546, -15, 15],
    [-484, 62, 15],
    [15, 218, 15],
    [687, 93, 15],
    [687, 93, 0],
    [578, -15, 15],
    [15, 179, 15],
    [515, 62, 15],
];

const JAVA_BANNER_FRAME_2: [[i32; 3]; 24] = [
    [-655, -53, 0],
    [-533, 61, 0],
    [-472, 61, 0],
    [-533, -15, 0],
    [564, -15, 0],
    [686, -53, 0],
    [-655, -53, 15],
    [-533, 61, 15],
    [-640, 91, 15],
    [-640, 91, 0],
    [15, 213, 0],
    [15, 175, 0],
    [503, 61, 0],
    [564, 61, 0],
    [564, 61, 15],
    [686, -53, 15],
    [-533, -15, 15],
    [-472, 61, 15],
    [15, 213, 15],
    [671, 91, 15],
    [671, 91, 0],
    [564, -15, 15],
    [15, 175, 15],
    [503, 61, 15],
];

const JAVA_BANNER_FRAME_3: [[i32; 3]; 24] = [
    [-639, -52, 0],
    [-520, 59, 0],
    [-461, 59, 0],
    [-520, -14, 0],
    [550, -14, 0],
    [669, -52, 0],
    [-639, -52, 14],
    [-520, 59, 14],
    [-624, 89, 14],
    [-624, 89, 0],
    [14, 208, 0],
    [14, 171, 0],
    [490, 59, 0],
    [550, 59, 0],
    [550, 59, 14],
    [669, -52, 14],
    [-520, -14, 14],
    [-461, 59, 14],
    [14, 208, 14],
    [654, 89, 14],
    [654, 89, 0],
    [550, -14, 14],
    [14, 171, 14],
    [490, 59, 14],
];

const JAVA_BANNER_FRAME_4: [[i32; 3]; 24] = [
    [-623, -50, 0],
    [-507, 58, 0],
    [-449, 58, 0],
    [-507, -14, 0],
    [536, -14, 0],
    [652, -50, 0],
    [-623, -50, 14],
    [-507, 58, 14],
    [-609, 87, 14],
    [-609, 87, 0],
    [14, 203, 0],
    [14, 166, 0],
    [478, 58, 0],
    [536, 58, 0],
    [536, 58, 14],
    [652, -50, 14],
    [-507, -14, 14],
    [-449, 58, 14],
    [14, 203, 14],
    [638, 87, 14],
    [638, 87, 0],
    [536, -14, 14],
    [14, 166, 14],
    [478, 58, 14],
];

const JAVA_BANNER_BBOXES: [[i32; 4]; 5] = [
    [10, 232, 488, 326],
    [15, 233, 482, 324],
    [21, 233, 477, 322],
    [26, 234, 471, 320],
    [32, 234, 465, 319],
];

fn client(revision: ClientRevision) -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: std::env::temp_dir()
                .join(format!("r377-render-hints-{}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            members: true,
            lowmem: true,
        },
        revision,
    );
    c.ingame = true;
    c.cam_x = 0;
    c.cam_y = 0;
    c.cam_z = 0;
    c.cam_pitch = 0;
    c.cam_yaw = 0;
    c
}

fn solid_sprite(rgb: i32) -> Pix32 {
    let mut sprite = Pix32::new(3, 3);
    sprite.data.fill(rgb);
    sprite
}

fn renderer() -> Renderer {
    let mut r = Renderer::new(false);
    let mut media = Media::empty();
    media.headicons[2] = Some(solid_sprite(OLD_NPC_HINT));
    media.headicons[7] = Some(solid_sprite(OLD_PLAYER_HINT));
    media.headicons_hint[0] = Some(solid_sprite(R377_NPC_HINT));
    media.headicons_hint[1] = Some(solid_sprite(R377_PLAYER_HINT));
    r.media = Arc::new(media);
    r.area_game = Some(PixMap::new(512, 334));
    r.pix3d.set_clipping(512, 334);
    r.pix3d.origin_x = 256;
    r.pix3d.origin_y = 167;
    r
}

fn pixels(r: &Renderer) -> &[i32] {
    &r.area_game.as_ref().expect("game surface").pixels
}

fn clear(r: &mut Renderer) {
    r.area_game.as_mut().expect("game surface").pixels.fill(0);
}

fn add_npc_hint_target(c: &mut Client) {
    c.hint_type = 1;
    c.hint_npc = 0;
    c.npc_count = 1;
    c.npc_ids = vec![0];
    let mut npc = ClientNpc::default();
    npc.entity.x = 384;
    npc.entity.z = 1280;
    npc.entity.height = 100;
    npc.r#type = Some(0);
    c.npc[0] = Some(Box::new(npc));
}

fn add_player_hint_target(c: &mut Client) {
    c.hint_type = 10;
    c.hint_player = 0;
    c.player_count = 1;
    c.player_ids[0] = 0;
    let mut player = ClientPlayer {
        ready: true,
        ..Default::default()
    };
    player.entity.x = 384;
    player.entity.z = 1280;
    player.entity.height = 100;
    c.players[0] = Some(Box::new(player));
}

#[test]
fn entity_hints_select_revision_377_sprites_and_keep_legacy_indices() {
    let mut r = renderer();
    let mut r377 = client(ClientRevision::R377);
    add_npc_hint_target(&mut r377);

    r377.loop_cycle = 0;
    r.entity_overlays(&mut r377);
    assert!(pixels(&r).contains(&R377_NPC_HINT));
    assert!(!pixels(&r).contains(&OLD_NPC_HINT));

    clear(&mut r);
    r377.loop_cycle = 10;
    r.entity_overlays(&mut r377);
    assert!(!pixels(&r).contains(&R377_NPC_HINT));

    clear(&mut r);
    let mut player_377 = client(ClientRevision::R377);
    add_player_hint_target(&mut player_377);
    r.entity_overlays(&mut player_377);
    assert!(pixels(&r).contains(&R377_PLAYER_HINT));
    assert!(!pixels(&r).contains(&OLD_PLAYER_HINT));

    for revision in [ClientRevision::R274, ClientRevision::R289] {
        clear(&mut r);
        let mut legacy = client(revision);
        add_npc_hint_target(&mut legacy);
        r.entity_overlays(&mut legacy);
        assert!(pixels(&r).contains(&OLD_NPC_HINT));
        assert!(!pixels(&r).contains(&R377_NPC_HINT));

        clear(&mut r);
        let mut legacy_player = client(revision);
        add_player_hint_target(&mut legacy_player);
        r.entity_overlays(&mut legacy_player);
        assert!(pixels(&r).contains(&OLD_PLAYER_HINT));
        assert!(!pixels(&r).contains(&R377_PLAYER_HINT));
    }
}

#[test]
fn hint_media_loading_requires_explicit_revision_377() {
    let Ok(cache) = std::env::var("CLIENT_377_CACHE_DIR") else {
        return;
    };
    let legacy = Media::process(&cache, ClientRevision::R289);
    assert!(legacy.headicons_hint.iter().all(Option::is_none));
    let r377 = Media::process(&cache, ClientRevision::R377);
    assert!(r377.headicons_hint[0].is_some());
    assert!(r377.headicons_hint[1].is_some());
    assert!(!Arc::ptr_eq(&legacy, &r377));
    assert!(Arc::ptr_eq(
        &r377,
        &Media::process(&cache, ClientRevision::R377)
    ));
}
struct NoopModelProvider;

impl ModelProvider for NoopModelProvider {
    fn request_model(&mut self, _id: i32) {}
}

fn vertices(model: &Model) -> Vec<[i32; 3]> {
    let x = model.point_x.as_deref().expect("decoded model x vertices");
    let y = model.point_y.as_deref().expect("decoded model y vertices");
    let z = model.point_z.as_deref().expect("decoded model z vertices");
    x.iter()
        .zip(y)
        .zip(z)
        .map(|((&x, &y), &z)| [x, y, z])
        .collect()
}

fn raster_bbox(pixels: &[i32], width: usize, background: i32) -> Option<[i32; 4]> {
    let mut bbox = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for (index, &pixel) in pixels.iter().enumerate() {
        if pixel == background {
            continue;
        }
        let x = (index % width) as i32;
        let y = (index / width) as i32;
        bbox[0] = bbox[0].min(x);
        bbox[1] = bbox[1].min(y);
        bbox[2] = bbox[2].max(x);
        bbox[3] = bbox[3].max(y);
    }
    (bbox[0] != i32::MAX).then_some(bbox)
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    png.extend_from_slice(&png_crc32(&png[crc_start..]).to_be_bytes());
}

fn encode_banner_png(pixels: &[i32]) -> Vec<u8> {
    use std::io::Write;

    const WIDTH: usize = 512;
    const HEIGHT: usize = 334;
    assert_eq!(pixels.len(), WIDTH * HEIGHT);

    let mut scanlines = Vec::with_capacity(HEIGHT * (WIDTH * 3 + 1));
    for row in pixels.as_chunks::<WIDTH>().0 {
        scanlines.push(0);
        for &pixel in row {
            scanlines.extend_from_slice(&[
                ((pixel >> 16) & 0xff) as u8,
                ((pixel >> 8) & 0xff) as u8,
                (pixel & 0xff) as u8,
            ]);
        }
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&scanlines)
        .expect("compress banner raster");
    let compressed = encoder.finish().expect("finish banner raster compression");

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(WIDTH as u32).to_be_bytes());
    header.extend_from_slice(&(HEIGHT as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    append_png_chunk(&mut png, b"IHDR", &header);
    append_png_chunk(&mut png, b"IDAT", &compressed);
    append_png_chunk(&mut png, b"IEND", &[]);
    png
}

fn write_banner_raster_evidence(
    frame_index: usize,
    frame_id: i32,
    pixels: &[i32],
    rust_bbox: [i32; 4],
    rust_vertices: &[[i32; 3]],
    java_bbox: [i32; 4],
    java_vertices: &[[i32; 3]],
) {
    let Some(directory) = std::env::var_os("CLIENT_377_RENDER_EVIDENCE_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).expect("create requested render evidence directory");
    std::fs::write(
        directory.join(format!("banner-rust-frame-{frame_index:02}.png")),
        encode_banner_png(pixels),
    )
    .expect("write requested render PNG evidence");

    let receipt = serde_json::json!({
        "frame_index": frame_index,
        "frame_id": frame_id,
        "rust_bbox": rust_bbox,
        "java_bbox": java_bbox,
        "rust_vertices": rust_vertices,
        "java_vertices": java_vertices,
    });
    std::fs::write(
        directory.join(format!("banner-rust-frame-{frame_index:02}.json")),
        serde_json::to_vec_pretty(&receipt).expect("serialize banner frame evidence"),
    )
    .expect("write requested render JSON evidence");
}

// The overlay's 8680 → layer 12227 → model 12228 offsets and the model's
// centre/zoom follow Client.java 9944-9970; the selected frame is the
// sequence's frame at the live IfType animation index.

#[test]
fn tutorial_banner_animation_and_projection_match_client_java_377() {
    let _models = MODEL_FIXTURE_LOCK.lock().unwrap();
    const ROOT_ID: usize = 8680;
    const LAYER_ID: usize = 12227;
    const MODEL_ID: usize = 12228;
    const MODEL_ID_377: i32 = 5201;
    const SEQUENCE_ID: usize = 1638;
    const BACKGROUND: i32 = 0x010203;

    assert_eq!(R377_BANNER_MODEL_5201.len(), 321);
    assert_eq!(R377_BANNER_ANIMS_399.len(), 70);
    assert_eq!(
        client::io::Packet::getcrc(R377_BANNER_MODEL_5201, 0, R377_BANNER_MODEL_5201.len()) as u32,
        0x3f3fe9f1
    );
    assert_eq!(
        client::io::Packet::getcrc(R377_BANNER_ANIMS_399, 0, R377_BANNER_ANIMS_399.len()) as u32,
        0x9130f1cd
    );

    Model::reset_for_tests();
    Model::init(5202, Box::new(NoopModelProvider));
    Model::unpack(MODEL_ID_377, Some(R377_BANNER_MODEL_5201));
    AnimFrame::init(12338);
    AnimFrame::unpack(R377_BANNER_ANIMS_399);
    assert!(AnimFrame::get(12335).is_some());

    let mut cache = Cache::default();
    cache.seqs.resize_with(SEQUENCE_ID + 1, SeqType::default);
    cache.seqs[SEQUENCE_ID] = SeqType {
        num_frames: 8,
        frames: Some(vec![12334, 12335, 12336, 12337, 12338, 12337, 12336, 12335]),
        iframes: Some(vec![-1; 8]),
        delay: Some(vec![0, 7, 7, 7, 7, 7, 7, 7]),
        loops: 8,
        ..SeqType::default()
    };

    let mut c = client(ClientRevision::R377);
    c.cache = Arc::new(cache);

    let root = IfType {
        id: ROOT_ID as i32,
        r#type: ComponentType::TYPE_LAYER,
        width: 512,
        height: 334,
        children: Some(vec![LAYER_ID as i32]),
        child_x: Some(vec![0]),
        child_y: Some(vec![206]),
        ..IfType::default()
    };
    c.set_iface(ROOT_ID, root);
    c.set_iface(
        LAYER_ID,
        IfType {
            id: LAYER_ID as i32,
            r#type: ComponentType::TYPE_LAYER,
            width: 512,
            height: 128,
            children: Some(vec![MODEL_ID as i32]),
            child_x: Some(vec![228]),
            child_y: Some(vec![29]),
            ..IfType::default()
        },
    );
    c.set_iface(
        MODEL_ID,
        IfType {
            id: MODEL_ID as i32,
            r#type: ComponentType::TYPE_MODEL,
            width: 32,
            height: 32,
            ..IfType::default()
        },
    );
    c.set_iface_mut(
        MODEL_ID,
        IfTypeMut {
            model1_type: 1,
            model1_id: MODEL_ID_377,
            model_zoom: 1500,
            model_anim: SEQUENCE_ID as i32,
            ..IfTypeMut::default()
        },
    );

    let mut r = Renderer::new(false);
    r.pix3d.set_clipping(512, 334);
    let frames = [
        (12334, &JAVA_BANNER_FRAME_0, JAVA_BANNER_BBOXES[0]),
        (12335, &JAVA_BANNER_FRAME_1, JAVA_BANNER_BBOXES[1]),
        (12336, &JAVA_BANNER_FRAME_2, JAVA_BANNER_BBOXES[2]),
        (12337, &JAVA_BANNER_FRAME_3, JAVA_BANNER_BBOXES[3]),
        (12338, &JAVA_BANNER_FRAME_4, JAVA_BANNER_BBOXES[4]),
        (12337, &JAVA_BANNER_FRAME_3, JAVA_BANNER_BBOXES[3]),
        (12336, &JAVA_BANNER_FRAME_2, JAVA_BANNER_BBOXES[2]),
        (12335, &JAVA_BANNER_FRAME_1, JAVA_BANNER_BBOXES[1]),
    ];
    for (frame_index, (frame_id, expected_vertices, expected_bbox)) in
        frames.into_iter().enumerate()
    {
        let model = c
            .if_(MODEL_ID)
            .expect("tutorial model component")
            .get_temp_model(&c.cache, None, frame_id, -1, false)
            .expect("decoded tutorial model");
        let actual_vertices = vertices(&model);
        assert_eq!(
            actual_vertices.as_slice(),
            expected_vertices.as_slice(),
            "Java 377 transformed vertices for frame {frame_id}"
        );

        c.overlay_mut(MODEL_ID)
            .expect("tutorial model overlay")
            .anim_frame = frame_index as i32;
        let mut pixels = vec![BACKGROUND; 512 * 334];
        {
            let mut surface = Pix2D::with_pixels(&mut pixels, 512, 334);
            r.draw_interface(&mut c, ROOT_ID as i32, 0, 0, 0, &mut surface);
        }
        let actual_bbox =
            raster_bbox(&pixels, 512, BACKGROUND).expect("tutorial banner rasterized");
        write_banner_raster_evidence(
            frame_index,
            frame_id,
            &pixels,
            actual_bbox,
            &actual_vertices,
            expected_bbox,
            expected_vertices,
        );
        assert_eq!(
            actual_bbox, expected_bbox,
            "Java 377 TYPE_MODEL projection for sequence frame {frame_index}"
        );
    }
}

fn morph_cache() -> Cache {
    Cache {
        locs: vec![
            LocType {
                id: 0,
                ..LocType::default()
            },
            LocType {
                id: 1,
                name: "Flour bin".into(),
                model: Some(vec![5201]),
                ..LocType::default()
            },
            LocType {
                id: 2,
                name: "Flour bin".into(),
                model: Some(vec![5201]),
                offsetx: 128,
                op: vec![Some("Empty".into())],
                ..LocType::default()
            },
        ],
        varps: vec![VarpType::default()],
        varbits: vec![VarBitType {
            basevar: 0,
            startbit: 1,
            endbit: 2,
            ..VarBitType::default()
        }],
        revision_377: Some(Box::new(CacheRevision377 {
            config: CacheFields377 {
                locs: vec![
                    LocFields377 {
                        morph: Some(MorphTable377 {
                            varbit: -1,
                            varp: 0,
                            targets: vec![1, 2, -1],
                        }),
                    },
                    LocFields377 { morph: None },
                    LocFields377 { morph: None },
                ],
                ..CacheFields377::default()
            },
            interfaces: InterfaceFields377 {
                client_code_layers: Default::default(),
                type1_fields: Vec::new(),
                null_string_masks: Vec::new(),
                initial_overlays: Vec::new(),
            },
        })),
        ..Cache::default()
    }
}

#[test]
fn location_morph_selectors_match_java_377_without_a_fallback() {
    // LocType.java:172-184: an indexed target, with varbit preferred over
    // varp and inclusive end bits through VARBIT_MASKS[end - start].
    let mut cache = morph_cache();
    assert_eq!(cache.resolve_loc(0, &[0]).unwrap().id, 1);
    assert_eq!(cache.resolve_loc(0, &[1]).unwrap().id, 2);
    for value in [-1, 2, 3, i32::MAX] {
        assert!(cache.resolve_loc(0, &[value]).is_none(), "selector {value}");
    }
    assert!(cache.resolve_loc(0, &[]).is_none());
    cache.revision_377.as_mut().unwrap().config.locs[0]
        .morph
        .as_mut()
        .unwrap()
        .varbit = 0;
    assert_eq!(cache.resolve_loc(0, &[0]).unwrap().id, 1);
    assert_eq!(cache.resolve_loc(0, &[2]).unwrap().id, 2);
    assert!(
        cache.resolve_loc(0, &[4]).is_none(),
        "last inclusive bit selects the sentinel"
    );
    assert!(
        cache.resolve_loc(0, &[6]).is_none(),
        "out-of-range has no final-target fallback"
    );
    cache.varbits[0].startbit = 0;
    cache.varbits[0].endbit = 31;
    assert_eq!(cache.resolve_loc(0, &[1]).unwrap().id, 2);
    assert!(cache.resolve_loc(0, &[-1]).is_none());
    cache.varbits[0].endbit = 32;
    assert!(cache.resolve_loc(0, &[0]).is_none());
    cache.revision_377 = None;
    assert_eq!(
        cache.resolve_loc(0, &[]).unwrap().id,
        0,
        "legacy cache keeps its root"
    );
}

#[test]
fn model_free_morph_roots_survive_map_and_zone_placement() {
    // Java World.java:127-141,804-819 stores ClientLocAnim before asking
    // the selected form for geometry. The root need not have any models.
    let groundh = vec![vec![vec![0; 5]; 5]; 4];
    let mapl = vec![vec![vec![0; 5]; 5]; 4];
    let mut world = World::new(vec![vec![vec![0; 5]; 5]; 4], 4, 4, 4);
    let mut collisions = std::array::from_fn(|_| CollisionMap::new());
    let mut build = ClientBuild::new();
    build.low_mem = false;
    for revision_377 in [false, true] {
        let mut cache = morph_cache();
        if !revision_377 {
            cache.revision_377 = None;
        }
        for shape in 0..=22 {
            for angle in 0..4 {
                for changed in [false, true] {
                    world.reset_map();
                    if changed {
                        ClientBuild::change_loc_unchecked(
                            &cache, &mut world, None, &groundh, 0, 1, 1, 0, shape, angle, 0, 0,
                        );
                    } else {
                        // Loc 0 at local (1,1), then placement/loc terminators.
                        let bytes = [1, 66, ((shape << 2) | angle) as u8, 0, 0];
                        build.load_locations(
                            &cache,
                            &mut world,
                            &mut collisions,
                            &groundh,
                            &mapl,
                            &bytes,
                            0,
                            0,
                            0,
                        );
                    }
                    let typecode = match shape {
                        0..=3 => world.wall_type(0, 1, 1),
                        4..=8 => world.decor_type(0, 1, 1),
                        22 => world.gd_type(0, 1, 1),
                        _ => world.scene_type(0, 1, 1),
                    };
                    assert_eq!(
                        typecode != 0,
                        revision_377,
                        "377={revision_377}, changed={changed}, shape={shape}, angle={angle}",
                    );
                }
            }
        }
    }
}

#[test]
fn location_morph_models_are_dynamic_without_an_animation() {
    let _models = MODEL_FIXTURE_LOCK.lock().unwrap();
    Model::reset_for_tests();
    Model::init(5202, Box::new(NoopModelProvider));
    Model::unpack(5201, Some(R377_BANNER_MODEL_5201));
    AnimFrame::init(12338);
    AnimFrame::unpack(R377_BANNER_ANIMS_399);
    let mut cache = morph_cache();
    let mut world = World::new(vec![vec![vec![0; 5]; 5]], 4, 1, 4);
    let typecode = 0x4000_0000 | 1 << 7 | 1;
    world.add_scenery(
        0,
        1,
        1,
        0,
        typecode,
        LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        0,
        0,
        0,
        0,
    );
    let index = world.last_sprite_index().unwrap();
    let mut render = RenderWorld::new();
    let Some(SceneModel::LocAnim(anim)) = render.sprite_model(&world, &cache, 0, index) else {
        panic!("opcode 77 requires a dynamic source even when the root has no models or sequence");
    };
    assert!(anim.anim.is_none());
    let (_, _, _, _, empty) = render
        .loc_model_at_with_varps(&world, &cache, 0, &[0], 0, 1, 1, 0)
        .unwrap();
    let (_, _, _, _, full) = render
        .loc_model_at_with_varps(&world, &cache, 1, &[1], 0, 1, 1, 0)
        .unwrap();
    let expected_full: Vec<_> = vertices(&empty)
        .into_iter()
        .map(|[x, y, z]| [x + 128, y, z])
        .collect();
    assert_eq!(
        vertices(&full),
        expected_full,
        "the cached scene slot follows live selectors"
    );
    assert!(render
        .loc_model_at_with_varps(&world, &cache, 2, &[2], 0, 1, 1, 0)
        .is_none());

    cache.seqs.push(SeqType {
        num_frames: 2,
        frames: Some(vec![12334, 12335]),
        delay: Some(vec![1, 1]),
        loops: 2,
        ..SeqType::default()
    });
    let mut animated = ClientLocAnim::new(
        &cache,
        0,
        LocShape::CENTREPIECE_STRAIGHT,
        0,
        0,
        0,
        0,
        0,
        0,
        false,
        0,
    );
    assert!(animated
        .get_temp_model_with_varps(&cache, 3, &[2])
        .is_none());
    assert_eq!(
        (animated.anim_frame, animated.anim_cycle),
        (1, 2),
        "Java advances the root sequence before hiding an absent form"
    );
    assert!(animated
        .get_temp_model_with_varps(&cache, 3, &[1])
        .is_some());
    assert_eq!(
        (animated.anim_frame, animated.anim_cycle),
        (1, 2),
        "changing the child does not restart the root sequence"
    );
}

#[test]
fn received_varp_changes_location_menu_but_keeps_java_scene_identity() {
    let mut c = client(ClientRevision::R377);
    c.cache = Arc::new(morph_cache());
    let typecode = 0x4000_0000 | 1 << 7 | 1;
    c.world
        .set_ground_decor(0, 1, 1, 0, typecode, 0, 0, 0, 0, 0);
    c.pick_count = 1;
    c.pick_typecodes[0] = typecode;
    c.add_world_options();
    assert!(!c.menu_option[..c.menu_num_entries as usize]
        .iter()
        .any(|op| op.starts_with("Empty")));

    // Client.java:7325-7340: id g2_alt2, signed value g1b_alt3.
    c.handle_packet(182, &mut Packet::new(vec![0, 128, 127]));
    assert!(c.ingame);
    assert_eq!((c.var[0], c.var_serv[0]), (1, 1));
    c.menu_num_entries = 0;
    c.add_world_options();
    let empty = (0..c.menu_num_entries as usize)
        .find(|&i| c.menu_action[i] == MiniMenuAction::OP_LOC1)
        .unwrap();
    assert_eq!(c.menu_option[empty], "Empty @cya@Flour bin");
    assert_eq!(
        c.menu_param_a[empty], typecode,
        "Java sends the original scene id for operations"
    );
    let examine = (0..c.menu_num_entries as usize)
        .find(|&i| c.menu_action[i] == MiniMenuAction::OP_LOC6)
        .unwrap();
    assert_eq!(
        c.menu_param_a[examine],
        2 << 14,
        "Examine identifies the selected definition"
    );

    c.handle_packet(182, &mut Packet::new(vec![0, 128, 126]));
    assert!(c.ingame);
    c.menu_num_entries = 0;
    c.add_world_options();
    assert_eq!(
        c.menu_num_entries, 1,
        "the absent target adds no location options"
    );
}
