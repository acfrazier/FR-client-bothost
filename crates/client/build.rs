use std::{env, fs, path::Path};

fn main() {
    let n = env::var("LOGIN_RSAN").unwrap_or_else(|_| {
        "7162900525229798032761816791230527296329313291232324290237849263501208207972894053929065636522363163621000728841182238772712427862772219676577293600221789".into()
    });
    let e = env::var("LOGIN_RSAE").unwrap_or_else(|_| {
        "58778699976184461502525193738213253649000149147835990136706041084440742975821".into()
    });
    assert!(n.chars().all(|c| c.is_ascii_digit()), "LOGIN_RSAN must be decimal");
    assert!(e.chars().all(|c| c.is_ascii_digit()), "LOGIN_RSAE must be decimal");
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("login_rsa_gen.rs");
    fs::write(
        out,
        format!(
            "pub const LOGIN_RSAN: &str = \"{n}\";\npub const LOGIN_RSAE: &str = \"{e}\";\n"
        ),
    )
    .unwrap();
    println!("cargo:rerun-if-env-changed=LOGIN_RSAN");
    println!("cargo:rerun-if-env-changed=LOGIN_RSAE");
}
