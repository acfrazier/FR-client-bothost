// Config types unpacked from the engine's `config` JAG archive (1:1 of the
// client-ts `config/` decode loops). Skipped when the engine pack is absent.
use client::client::{Client, ClientConfig};
use client::config::if_type::IfType;
use client::config::{Cache, ObjType};
use client::io::JagFile;

fn engine_config_jag() -> Option<JagFile> {
    let path = std::env::var("ENGINE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/experiments/Server/engine",
            std::env::var("HOME").unwrap()
        )
    });
    let bytes = std::fs::read(format!("{path}/data/pack/client/config")).ok()?;
    Some(JagFile::new(bytes))
}

#[test]
fn unpack_obj_from_engine_config_jag() {
    let path = std::env::var("ENGINE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/experiments/Server/engine",
            std::env::var("HOME").unwrap()
        )
    });
    let bytes = match std::fs::read(format!("{path}/data/pack/client/config")) {
        Ok(b) => b,
        Err(_) => return,
    };
    let jag = JagFile::new(bytes);
    let objs = ObjType::unpack(&jag);
    assert!(objs.len() > 1);
    assert!(objs.iter().any(|o| o.name.eq_ignore_ascii_case("bones")));
}

#[test]
fn cache_holds_all_unpacked_tables() {
    let Some(jag) = engine_config_jag() else {
        return;
    };
    let cache = Cache::unpack(&jag);
    assert!(cache.objs.len() > 1);
    assert!(cache.npcs.len() > 1);
    assert!(cache.locs.len() > 1);
    assert!(cache.flos.len() > 1);
    assert!(cache.idks.len() > 1);
    assert!(cache.seqs.len() > 1);
    assert!(cache.spots.len() > 1);
    assert!(cache.varbits.len() > 1);
    assert!(cache.varps.len() > 1);
}

#[test]
fn cache_obj_by_id_matches_unpacked_table() {
    let Some(jag) = engine_config_jag() else {
        return;
    };
    let cache = Cache::unpack(&jag);
    let id = cache
        .objs
        .iter()
        .position(|o| o.name.eq_ignore_ascii_case("bones"))
        .unwrap();
    assert!(cache.obj(id).name.eq_ignore_ascii_case("bones"));
    // spot anims resolve their seq index once both tables are loaded
    let linked = cache.spots.iter().filter(|s| s.seq.is_some()).count();
    assert!(linked > 0);
}

#[test]
fn client_owns_an_empty_cache_until_unpacked() {
    // Private scratch dir: `/tmp` is the live cache that `run`'s `maininit`
    // fetch loop persists jags into, so it is not reliably empty.
    let dir = std::env::temp_dir().join("274-empty-cache");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    });
    assert!(c.cache.objs.is_empty());
    assert!(!c.error_loading);
    let Some(jag) = engine_config_jag() else {
        return;
    };
    c.cache = Cache::unpack(&jag);
    assert!(c
        .cache
        .objs
        .iter()
        .any(|o| o.name.eq_ignore_ascii_case("bones")));
}

#[test]
fn client_new_unpacks_config_from_cache_dir() {
    let path = std::env::var("ENGINE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/experiments/Server/engine",
            std::env::var("HOME").unwrap()
        )
    });
    let cache_dir = format!("{path}/data/pack/client");
    if !std::path::Path::new(&format!("{cache_dir}/config")).is_file() {
        return;
    }
    let c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir,
        members: true,
        lowmem: false,
    });
    assert!(!c.error_loading);
    assert!(c
        .cache
        .objs
        .iter()
        .any(|o| o.name.eq_ignore_ascii_case("bones")));
}

#[test]
fn missing_config_jag_sets_error_loading() {
    let dir = std::env::temp_dir().join(format!("274-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("title"), b"not-a-jag").unwrap();
    let c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_string_lossy().into(),
        members: true,
        lowmem: false,
    });
    let _ = std::fs::remove_dir_all(&dir);
    assert!(c.error_loading);
    assert!(c.cache.objs.is_empty());
    assert_eq!(c.shell.deltime, 1000);
}

fn engine_interface_jag() -> Option<JagFile> {
    let path = std::env::var("ENGINE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/experiments/Server/engine",
            std::env::var("HOME").unwrap()
        )
    });
    let bytes = std::fs::read(format!("{path}/data/pack/client/interface")).ok()?;
    Some(JagFile::new(bytes))
}

#[test]
fn iftype_unpack_keeps_inv_background_names() {
    let Some(jag) = engine_interface_jag() else {
        return;
    };
    let ifaces = IfType::unpack(&jag);
    assert!(
        ifaces.iter().flatten().any(|c| c
            .inv_background_name
            .as_ref()
            .is_some_and(|v| v.iter().any(|n| n.is_some()))),
        "a TYPE_INV component should keep its inv-background sprite names"
    );
}

/// Strip the pack obj's render fields down to the deterministic synthetic
/// triangle (model 0, identity angles, no cert/recolour/resize), keeping the
/// pack-decoded identity/name/op fields: the sprite must resolve purely from
/// the engine pack's config jag without the OnDemand model stream.
fn renderable(obj: ObjType) -> ObjType {
    ObjType {
        model: 0,
        zoom2d: 2000,
        xan2d: 0,
        yan2d: 0,
        zan2d: 0,
        xof2d: 0,
        yof2d: 0,
        stackable: false,
        resizex: 128,
        resizey: 128,
        resizez: 128,
        ambient: 0,
        contrast: 0,
        recol_s: None,
        recol_d: None,
        countobj: None,
        countco: None,
        certlink: -1,
        certtemplate: -1,
        ..obj
    }
}

#[test]
fn get_sprite_resolves_engine_pack_obj() {
    let Some(jag) = engine_config_jag() else {
        return;
    };
    let mut cache = Cache::unpack(&jag);
    if cache.objs.len() < 4 {
        return;
    }
    use client::dash3d::Model;
    use client::graphics::{Pix3D, Pix3DDraw};

    // One 3-vertex, 1-face triangle (same byte-crafted model as the
    // obj_type.rs unit tests): registered as model 0 so get_sprite can
    // render without OnDemand.
    const MODEL: &[u8] = &[
        7, 7, 7, // vertex order: x+y+z deltas for each of 3 vertices
        1, // face index order: a,b,c are all deltas
        0x40, 0x41, 0x41, // face index deltas: a=0, b=1, c=2 (cumulative)
        0x00, 0xFF, // face colour (HSL 255)
        0x40, 0x68, 0x18, // vertexX deltas: 0, +40, -40
        0x68, 0x40, 0x18, // vertexY deltas: +40, 0, -40
        0x40, 0x40, 0x40, // vertexZ deltas: 0, 0, 0
        0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3, 0, 3, // trailer
    ];
    Model::unpack(0, Some(MODEL));
    Pix3D::init_colour_table(0.8);
    let mut pix = Pix3DDraw::default();

    // non-stackable pack obj without countobj: owi 32, ohi -1, and the
    // render arm produces a non-empty sprite
    cache.objs[1] = renderable(cache.objs[1].clone());
    let s = ObjType::get_sprite(&cache, &mut pix, 1, 0, 5)
        .expect("a pack obj with a resolvable model must render a sprite");
    assert_eq!(s.owi, 32, "non-stackable -> owi 32");
    assert_eq!(s.ohi, -1, "countobj null forces ohi -1");
    assert!(s.data.iter().any(|&p| p != 0), "the sprite must contain rendered pixels");

    // countco walk onto a stackable variant: owi comes from the *resolved*
    // obj (33) while ohi stays the requested count
    let mut countobj = vec![0u16; 10];
    let mut countco = vec![0u16; 10];
    countobj[0] = 3;
    countco[0] = 5;
    cache.objs[2] = renderable(cache.objs[2].clone());
    cache.objs[2].countobj = Some(countobj);
    cache.objs[2].countco = Some(countco);
    cache.objs[3] = renderable(cache.objs[3].clone());
    cache.objs[3].stackable = true;
    let s = ObjType::get_sprite(&cache, &mut pix, 2, 0, 5)
        .expect("the countco-walked variant must render a sprite");
    assert_eq!(s.owi, 33, "owi comes from the resolved (stackable) variant");
    assert_eq!(s.ohi, 5, "ohi stays the requested count");
}
