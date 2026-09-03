#[test]
fn java_and_prod_pairs_are_distinct_decimal() {
    assert_eq!(client::JAVA_LOGIN_RSAN, "7162900525229798032761816791230527296329313291232324290237849263501208207972894053929065636522363163621000728841182238772712427862772219676577293600221789");
    assert_ne!(client::JAVA_LOGIN_RSAN, client::PROD_LOGIN_RSAN);
    assert_eq!(client::PROD_LOGIN_RSAE, "65537");
    assert!(client::PROD_LOGIN_RSAN.len() >= 250);
}

#[test]
fn garbage_login_rsan_falls_back_to_java_defaults() {
    let prev_n = std::env::var("LOGIN_RSAN").ok();
    let prev_e = std::env::var("LOGIN_RSAE").ok();
    let prev_target = std::env::var("BOT_TARGET").ok();
    std::env::set_var("BOT_TARGET", "local");
    std::env::set_var("LOGIN_RSAN", "not-a-modulus");
    std::env::set_var("LOGIN_RSAE", "65537");
    let (n, e) = client::login_rsa::active_biguints();
    match prev_n {
        Some(v) => std::env::set_var("LOGIN_RSAN", v),
        None => std::env::remove_var("LOGIN_RSAN"),
    }
    match prev_e {
        Some(v) => std::env::set_var("LOGIN_RSAE", v),
        None => std::env::remove_var("LOGIN_RSAE"),
    }
    match prev_target {
        Some(v) => std::env::set_var("BOT_TARGET", v),
        None => std::env::remove_var("BOT_TARGET"),
    }
    assert_eq!(
        n.to_string(),
        client::JAVA_LOGIN_RSAN,
        "garbage LOGIN_RSAN must not panic"
    );
    assert_eq!(e.to_string(), client::JAVA_LOGIN_RSAE);
}
