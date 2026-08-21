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
