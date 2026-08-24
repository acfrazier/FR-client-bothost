// TARGET-driven login RSA bake, shared between `build.rs` (bake time)
// and the crate (`login_rsa::resolve_rsa`, unit-tested). `build.rs`
// includes this file and feeds it the process environment; unit tests
// feed it a fake env map. Never fetched from the network at bake time.

/// The 274 engine's baked default RSA public half (the 2007scape key the
/// local `data/config/private.pem` pairs with). `TARGET=local` bakes this
/// when `LOCAL_RSAN`/`LOGIN_RSAE` are unset.
pub const JAVA_N: &str = "7162900525229798032761816791230527296329313291232324290237849263501208207972894053929065636522363163621000728841182238772712427862772219676577293600221789";
pub const JAVA_E: &str = "58778699976184461502525193738213253649000149147835990136706041084440742975821";

/// Resolve the login RSA public half for a build `target` from `env`.
///
/// - `local` (default): `LOCAL_RSAN`/`LOGIN_RSAN` or [`JAVA_N`]; exponent
///   `LOGIN_RSAE` or [`JAVA_E`] (the local engine's pair).
/// - `live`: requires a non-empty `LIVE_RSAN` (the modulus rs2b2t's
///   `client.js` currently serves) — `Err` without one so a live bake
///   cannot silently ship the local key.
/// - `prod`: requires a non-empty `PROD_RSAN`.
///
/// The live/prod exponent defaults to 65537 (rs2b2t's login RSA exponent,
/// not the Java default); `LOGIN_RSAE` overrides any target.
pub fn resolve_rsa(
    target: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(String, String), &'static str> {
    let n = match target {
        "live" => match env("LIVE_RSAN").filter(|v| !v.is_empty()) {
            Some(v) => v,
            None => return Err("TARGET=live requires LIVE_RSAN"),
        },
        "prod" => match env("PROD_RSAN").filter(|v| !v.is_empty()) {
            Some(v) => v,
            None => return Err("TARGET=prod requires PROD_RSAN"),
        },
        _ => env("LOCAL_RSAN")
            .filter(|v| !v.is_empty())
            .or_else(|| env("LOGIN_RSAN").filter(|v| !v.is_empty()))
            .unwrap_or_else(|| JAVA_N.into()),
    };
    let e = env("LOGIN_RSAE")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| match target {
            "live" | "prod" => "65537".into(),
            _ => JAVA_E.into(),
        });
    Ok((n, e))
}
