// Config types unpacked from the engine's `config` JAG archive (1:1 of the
// client-ts `config/` decode loops). Skipped when the engine pack is absent.
use client::client::{Client, ClientConfig};
use client::config::{Cache, ObjType};
use client::io::JagFile;

fn engine_config_jag() -> Option<JagFile> {
    let path = std::env::var("ENGINE_DIR")
        .unwrap_or_else(|_| format!("{}/experiments/Server/engine", std::env::var("HOME").unwrap()));
    let bytes = std::fs::read(format!("{path}/data/pack/client/config")).ok()?;
    Some(JagFile::new(bytes))
}

#[test]
fn unpack_obj_from_engine_config_jag() {
    let path = std::env::var("ENGINE_DIR").unwrap_or_else(|_| {
        format!("{}/experiments/Server/engine", std::env::var("HOME").unwrap())
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
    let Some(jag) = engine_config_jag() else { return };
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
    let Some(jag) = engine_config_jag() else { return };
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
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    assert!(c.cache.objs.is_empty());
    let Some(jag) = engine_config_jag() else { return };
    c.cache = Cache::unpack(&jag);
    assert!(c.cache.objs.iter().any(|o| o.name.eq_ignore_ascii_case("bones")));
}
