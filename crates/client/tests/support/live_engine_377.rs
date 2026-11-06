//! Opt-in live proofs must name Fairy Ring's engine before reading its RSA key.
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

pub fn engine_dir(value: Option<&OsStr>) -> Result<PathBuf, String> {
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or("ENGINE_DIR is required for a 377 live proof")?;
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("ENGINE_DIR must be an absolute Fairy Ring engine directory".into());
    }
    // Check the resolved path, not a symlink's name. In particular, Cargo's
    // legacy Engine A default is rejected before any private.pem is read.
    let path = path
        .canonicalize()
        .map_err(|error| format!("ENGINE_DIR: {error}"))?;
    if path.file_name() != Some(OsStr::new("engine"))
        || !path
            .components()
            .any(|part| part == Component::Normal(OsStr::new("fairy-ring-worktrees")))
    {
        return Err(
            "ENGINE_DIR must select Fairy Ring's own engine worktree, never a 274bot engine".into(),
        );
    }
    Ok(path)
}

pub fn require_engine(evidence: &Path, user: &str, cache: &str) -> PathBuf {
    let prefix = std::env::var("CLIENT_377_LIVE_PREFIX").unwrap_or_else(|_| "t2a".into());
    assert!(
        account_matches_prefix(user, &prefix),
        "account must start with nonempty CLIENT_377_LIVE_PREFIX (default t2a)"
    );
    let value = std::env::var_os("ENGINE_DIR");
    let engine = engine_dir(value.as_deref()).expect("explicit Fairy Ring ENGINE_DIR required");
    assert!(
        std::env::var_os("LOGIN_RSAN").is_none() && std::env::var_os("LOGIN_RSAE").is_none(),
        "377 live proofs derive the RSA key only from the selected ENGINE_DIR"
    );
    let key = engine.join("data/config/private.pem");
    client::login_rsa::rsa_from_pkcs1_pem_file(&key)
        .expect("selected engine RSA key must exist and decode, with no fallback");
    assert_eq!(client::engine_dir().canonicalize().unwrap(), engine);
    std::fs::create_dir_all(evidence).unwrap();
    let receipt = serde_json::json!({
        "engine_dir": engine,
        "rsa_key_path": key,
        "user": user,
        "account_prefix": prefix,
        "runtime_cache": cache,
        "host": "127.0.0.1", "http_port": 81, "game_port": 43595,
        "rsa_source": "mandatory ENGINE_DIR/data/config/private.pem; no overrides or fallback"
    });
    std::fs::write(
        evidence.join("live-config.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("377 engine receipt: {receipt}");
    engine
}

fn account_matches_prefix(user: &str, prefix: &str) -> bool {
    !prefix.is_empty() && user.starts_with(prefix)
}

#[test]
fn live_account_prefix_is_explicit_and_nonempty() {
    assert!(account_matches_prefix("t2atest", "t2a"));
    assert!(account_matches_prefix("s12fclose", "s12f"));
    assert!(!account_matches_prefix("t2atest", "s12f"));
    assert!(!account_matches_prefix("s12fclose", ""));
}

#[test]
fn live_engine_requires_explicit_absolute_path() {
    assert!(engine_dir(None).unwrap_err().contains("required"));
    assert!(engine_dir(Some(OsStr::new("")))
        .unwrap_err()
        .contains("required"));
    assert!(engine_dir(Some(OsStr::new("engine")))
        .unwrap_err()
        .contains("absolute"));
}

#[test]
fn live_engine_rejects_non_fairy_ring_directory_before_key_access() {
    // The repository exists, but its name is not an authorized engine worktree.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(engine_dir(Some(repo.as_os_str()))
        .unwrap_err()
        .contains("Fairy Ring"));
}
