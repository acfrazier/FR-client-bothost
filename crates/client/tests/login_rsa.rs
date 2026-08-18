const JAVA_N: &str = "7162900525229798032761816791230527296329313291232324290237849263501208207972894053929065636522363163621000728841182238772712427862772219676577293600221789";
const JAVA_E: &str = "58778699976184461502525193738213253649000149147835990136706041084440742975821";

#[test]
fn baked_rsa_defaults_to_java_literals_when_env_unset() {
    if std::env::var("LOGIN_RSAN").is_ok() {
        // rebuilt by redeploy.sh — still must be decimal digits
        assert!(client::LOGIN_RSAN.chars().all(|c| c.is_ascii_digit()));
        assert!(client::LOGIN_RSAE.chars().all(|c| c.is_ascii_digit()));
        return;
    }
    assert_eq!(client::LOGIN_RSAN, JAVA_N);
    assert_eq!(client::LOGIN_RSAE, JAVA_E);
}
