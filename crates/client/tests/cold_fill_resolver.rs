//! The standalone-native cold cache fill, stepped and one-call, against a
//! slow or failing system resolver.
//!
//! This test executable defines its own `getaddrinfo`, which the standard
//! library's name lookup calls in place of libc's. A name that ends in
//! [`SLOW_SUFFIX`] resolves to 127.0.0.1 after [`LOOKUP_DELAY`].
//! [`FAILED_HOST`] and [`FAILED_ASSET_HOST`] fail at once. Every other
//! name goes to libc's `getaddrinfo` unchanged. No system resolver setting
//! changes, and no external server is contacted.
//!
//! The standard library has no non-blocking or cancellable lookup, so a
//! lookup step can run past its deadline by the resolver's time. These
//! tests show that this costs at most one step per host in a fill, that
//! the fill reports each such step to its observer once, and that a cancel
//! never waits for a lookup. A failed lookup fails the fill at once, and
//! the error names the host in both fills.
#![cfg(all(target_os = "linux", feature = "standalone-native-transport"))]

#[path = "support/startup_snapshot.rs"]
mod support;

use client::io::{ClientRevision, OnDemand};
use client::unpack::{
    prepare_runtime_cache, RuntimeCacheFill, RuntimeCacheRequest, RuntimeCacheStep, SlowLookup,
};
use client::Transport;
use std::ffi::{c_char, c_int, CStr};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use support::{HttpServer, PacedEntryServer, PackSet, TempDir, PACK_NAMES};

/// Names with this suffix resolve slowly to 127.0.0.1.
const SLOW_SUFFIX: &str = ".cold-fill-test.invalid";
/// The lookup of this name fails at once.
const FAILED_HOST: &str = "no-game.cold-fill-failed.invalid";
/// The lookup of this name fails at once.
const FAILED_ASSET_HOST: &str = "no-asset.cold-fill-failed.invalid";
/// How long one slow lookup takes.
const LOOKUP_DELAY: Duration = Duration::from_millis(350);
/// The host's step deadline (`host-play` `TEMPLATE_STEP_WAIT`).
const HOST_STEP: Duration = Duration::from_millis(8);
/// The stated per-step bound of the stepped fill.
const STEP_BOUND: Duration = Duration::from_millis(250);

/// Every slow or failed name looked up so far, once per lookup.
static SLOW_LOOKUPS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn lookups_of(name: &str) -> usize {
    SLOW_LOOKUPS
        .lock()
        .unwrap()
        .iter()
        .filter(|looked_up| *looked_up == name)
        .count()
}

/// Every slow lookup step that a fill reported: role, host and lookup time.
static NOTICES: Mutex<Vec<(&'static str, String, Duration)>> = Mutex::new(Vec::new());

fn record_notice(lookup: &SlowLookup<'_>) {
    assert!(lookup.past_deadline > Duration::ZERO);
    NOTICES
        .lock()
        .unwrap()
        .push((lookup.role, lookup.host.to_string(), lookup.took));
}

/// The reported slow lookup steps of `host`: role and lookup time.
fn notices_of(host: &str) -> Vec<(&'static str, Duration)> {
    NOTICES
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, name, _)| name == host)
        .map(|(role, _, took)| (*role, *took))
        .collect()
}

/// One report of `host` in `role`, with the resolver's delay.
fn assert_one_notice(host: &str, role: &str) {
    let notices = notices_of(host);
    assert_eq!(notices.len(), 1, "{host}: {notices:?}");
    assert_eq!(notices[0].0, role);
    assert!(notices[0].1 >= LOOKUP_DELAY, "{host}: {notices:?}");
}

/// The resolver of this test executable (see the module documentation).
///
/// # Safety
///
/// The arguments are those of libc's `getaddrinfo`, and they go to it.
#[no_mangle]
pub unsafe extern "C" fn getaddrinfo(
    node: *const c_char,
    service: *const c_char,
    hints: *const libc::addrinfo,
    res: *mut *mut libc::addrinfo,
) -> c_int {
    type Getaddrinfo = unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const libc::addrinfo,
        *mut *mut libc::addrinfo,
    ) -> c_int;
    // SAFETY: libc's `getaddrinfo` has this signature.
    let libc_getaddrinfo: Getaddrinfo = unsafe {
        let symbol = libc::dlsym(libc::RTLD_NEXT, c"getaddrinfo".as_ptr());
        assert!(!symbol.is_null(), "libc getaddrinfo");
        std::mem::transmute::<*mut libc::c_void, Getaddrinfo>(symbol)
    };
    let mut node = node;
    if !node.is_null() {
        // SAFETY: a non-null `node` is a C string (`getaddrinfo` contract).
        let name = unsafe { CStr::from_ptr(node) }.to_string_lossy();
        if name.ends_with(SLOW_SUFFIX) {
            SLOW_LOOKUPS.lock().unwrap().push(name.into_owned());
            std::thread::sleep(LOOKUP_DELAY);
            node = c"127.0.0.1".as_ptr();
        } else if name == FAILED_HOST || name == FAILED_ASSET_HOST {
            SLOW_LOOKUPS.lock().unwrap().push(name.into_owned());
            // SAFETY: `res` is the caller's result slot (`getaddrinfo`
            // contract).
            unsafe { *res = std::ptr::null_mut() };
            return libc::EAI_NONAME;
        }
    }
    // SAFETY: the caller's arguments, with `node` a C string.
    unsafe { libc_getaddrinfo(node, service, hints, res) }
}

fn request<'a>(
    source: &'a Path,
    root: &'a Path,
    asset_host: &'a str,
    asset_port: u16,
    game_host: &'a str,
    game_port: u16,
) -> RuntimeCacheRequest<'a> {
    RuntimeCacheRequest {
        revision: ClientRevision::R289,
        transport: Transport::Tcp,
        jag_source: source,
        snapshot_root: root,
        asset_host,
        asset_port,
        game_host,
        game_port,
    }
}

/// A game host that the resolver cannot look up fails the fill at once,
/// after one lookup and without a worker. The error is a connection
/// failure that names the host, so the caller can pass over that name.
#[test]
fn a_failed_game_host_lookup_fails_the_fill_at_once() {
    let packs = PackSet::with_entries(37, [3, 0, 0, 0]);
    let http = HttpServer::start(packs.clone());
    let source = TempDir::in_memory("resolver-failed-source");
    packs.write_to(source.path()).unwrap();
    let root = TempDir::in_memory("resolver-failed-root");
    let mut fill = RuntimeCacheFill::begin(&request(
        source.path(),
        root.path(),
        "127.0.0.1",
        http.port(),
        FAILED_HOST,
        1,
    ));
    let started = Instant::now();
    let error = loop {
        assert!(started.elapsed() < Duration::from_secs(5), "no failure");
        match fill.step(Some(Instant::now() + HOST_STEP)) {
            Ok(RuntimeCacheStep::Ready(_)) => panic!("ready without the game host"),
            Ok(RuntimeCacheStep::Continue { .. }) => {}
            Err(error) => break error,
        }
    };
    let elapsed = started.elapsed();
    eprintln!("failed game-host lookup: {elapsed:?}: {error}");
    assert!(error.is_connection());
    assert_eq!(error.failed_lookup(), Some(FAILED_HOST));
    assert!(error.to_string().contains("name lookup failed"), "{error}");
    assert_eq!(lookups_of(FAILED_HOST), 1);
    assert_eq!(OnDemand::live_workers_for(FAILED_HOST, 1), 0);
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

/// An asset host that the resolver cannot look up fails the one-call fill
/// at once, after one lookup. The error is a connection failure that names
/// the host, as in the stepped fill, so the caller's world fallback can
/// pass over that name. The error text is that of a failed `/crc` request.
#[test]
fn a_failed_asset_host_lookup_names_the_host_in_the_one_call_fill() {
    let source = TempDir::in_memory("resolver-one-call-failed-source");
    let root = TempDir::in_memory("resolver-one-call-failed-root");
    let started = Instant::now();
    let error = prepare_runtime_cache(&request(
        source.path(),
        root.path(),
        FAILED_ASSET_HOST,
        1,
        "127.0.0.1",
        1,
    ))
    .expect_err("the asset host lookup fails");
    let elapsed = started.elapsed();
    eprintln!("one-call failed asset-host lookup: {elapsed:?}: {error}");
    assert!(error.is_connection());
    assert_eq!(error.failed_lookup(), Some(FAILED_ASSET_HOST));
    assert_eq!(error.to_string(), "update server /crc: connection problem");
    assert_eq!(lookups_of(FAILED_ASSET_HOST), 1);
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

/// The one-call fill gets `/crc` from the addresses of its one asset-host
/// lookup and completes. The source holds the packs, so no JAG download
/// looks the name up again.
#[test]
fn the_one_call_fill_looks_up_the_asset_host_once_for_crc() {
    let asset_host = "one-call-asset.cold-fill-test.invalid";
    let packs = PackSet::with_entries(37, [20, 5, 2, 5]);
    let http = HttpServer::start(packs.clone());
    let entries = PacedEntryServer::start(packs.entries.clone(), Duration::ZERO, Duration::ZERO);
    let source = TempDir::in_memory("resolver-one-call-source");
    packs.write_to(source.path()).unwrap();
    let root = TempDir::in_memory("resolver-one-call-root");
    let started = Instant::now();
    let prepared = prepare_runtime_cache(&request(
        source.path(),
        root.path(),
        asset_host,
        http.port(),
        "127.0.0.1",
        entries.port(),
    ))
    .unwrap();
    eprintln!(
        "one-call named asset host: {:?}; source {}; /crc requests {}; lookups {}",
        started.elapsed(),
        prepared.source,
        http.crc_count(),
        lookups_of(asset_host)
    );
    assert_eq!(prepared.source, "update-server");
    assert_eq!(http.crc_count(), 1);
    assert_eq!(
        lookups_of(asset_host),
        1,
        "the asset host is looked up once"
    );
}

/// A game host that is slow to look up does not delay a cancel: dropping
/// the fill 50 ms after the lookup started (a worker that looked up the
/// name itself would still be in it) returns at once, and no worker is
/// left. The update server takes the connection but never answers.
#[test]
fn cancelling_a_fill_does_not_wait_for_a_slow_name_lookup() {
    let game_host = "cancel-game.cold-fill-test.invalid";
    let packs = PackSet::with_entries(37, [3, 0, 0, 0]);
    let http = HttpServer::start(packs.clone());
    let source = TempDir::in_memory("resolver-cancel-source");
    packs.write_to(source.path()).unwrap();
    let root = TempDir::in_memory("resolver-cancel-root");
    // The kernel completes the connect; nothing ever answers the handshake.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let game_port = listener.local_addr().unwrap().port();
    let mut fill = RuntimeCacheFill::begin(&request(
        source.path(),
        root.path(),
        "127.0.0.1",
        http.port(),
        game_host,
        game_port,
    ));
    fill.observe_slow_lookups(record_notice);
    // Up to the step that starts the worker.
    while !matches!(
        fill.step(Some(Instant::now() + HOST_STEP)).unwrap(),
        RuntimeCacheStep::Continue { total: Some(_), .. }
    ) {}
    assert_eq!(OnDemand::live_workers_for(game_host, game_port), 1);
    // Fill steps until the game host's lookup has started, then 50 ms more.
    let waited = Instant::now();
    while lookups_of(game_host) == 0 {
        assert!(waited.elapsed() < Duration::from_secs(2), "no lookup");
        fill.step(Some(Instant::now() + HOST_STEP)).unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));

    let started = Instant::now();
    drop(fill);
    let cancel = started.elapsed();
    let lookups = lookups_of(game_host);
    eprintln!("cancel with a slow game-host lookup: drop {cancel:?}, {lookups} lookup(s)");
    assert!(
        cancel < Duration::from_millis(100),
        "cancel waited for the name lookup: {cancel:?}"
    );
    assert_eq!(lookups, 1, "the game host is looked up once");
    assert_one_notice(game_host, "game host");
    assert_eq!(OnDemand::live_workers_for(game_host, game_port), 0);
}

/// Step `fill` with the host's deadline until it is ready; the result, the
/// steps at or above the bound and the number of steps.
fn step_to_ready(
    fill: &mut RuntimeCacheFill,
) -> (
    std::sync::Arc<client::unpack::PreparedRuntimeCache>,
    Vec<Duration>,
    usize,
) {
    let started = Instant::now();
    let mut slow = Vec::new();
    let mut steps = 0;
    loop {
        assert!(started.elapsed() < Duration::from_secs(20), "fill stalled");
        let step_started = Instant::now();
        let step = fill.step(Some(step_started + HOST_STEP)).unwrap();
        let took = step_started.elapsed();
        if took >= STEP_BOUND {
            slow.push(took);
        }
        steps += 1;
        if let RuntimeCacheStep::Ready(prepared) = step {
            return (prepared, slow, steps);
        }
    }
}

/// Fill from an empty source, so that `/crc`, all eight JAG packs and the
/// entries come from the servers, with the given host names.
fn fill_with_hosts(label: &str, asset_host: &str, game_host: &str) -> (Vec<Duration>, usize) {
    let packs = PackSet::with_entries(37, [20, 5, 2, 5]);
    let http = HttpServer::start(packs.clone());
    let entries = PacedEntryServer::start(packs.entries.clone(), Duration::ZERO, Duration::ZERO);
    let source = TempDir::in_memory(&format!("{label}-source"));
    let root = TempDir::in_memory(&format!("{label}-root"));
    let mut fill = RuntimeCacheFill::begin(&request(
        source.path(),
        root.path(),
        asset_host,
        http.port(),
        game_host,
        entries.port(),
    ));
    fill.observe_slow_lookups(record_notice);
    let started = Instant::now();
    let (prepared, slow, steps) = step_to_ready(&mut fill);
    let total = started.elapsed();
    eprintln!(
        "{label}: {steps} steps in {total:?}; steps at or above {STEP_BOUND:?}: {slow:?}; \
         lookups: asset host {}, game host {}",
        lookups_of(asset_host),
        lookups_of(game_host)
    );
    assert_eq!(prepared.source, "update-server");
    assert_eq!(http.crc_count(), 1);
    for name in PACK_NAMES {
        assert_eq!(http.jag_count(name), 1, "{name} downloaded once");
    }
    assert_eq!(prepared.fetched.len(), PACK_NAMES.len());
    (slow, steps)
}

/// A slow asset host and a slow game host each delay at most one step: the
/// fill looks up each name once, keeps the addresses for `/crc`, all JAG
/// packs and the worker, reports each slow step once, and completes.
#[test]
fn a_slow_name_lookup_delays_at_most_one_step_per_host() {
    let asset_host = "asset.cold-fill-test.invalid";
    let game_host = "game.cold-fill-test.invalid";
    let (slow, _) = fill_with_hosts("two-hosts", asset_host, game_host);
    assert_eq!(
        lookups_of(asset_host),
        1,
        "the asset host is looked up once"
    );
    assert_eq!(lookups_of(game_host), 1, "the game host is looked up once");
    assert!(slow.len() <= 2, "steps at or above the bound: {slow:?}");
    assert_one_notice(asset_host, "asset host");
    assert_one_notice(game_host, "game host");
}

/// When the asset and game hosts are the same name, the fill looks it up
/// once, at most one step runs past the bound, and that step is reported
/// once.
#[test]
fn a_shared_host_is_looked_up_once_per_fill() {
    let host = "shared.cold-fill-test.invalid";
    let (slow, _) = fill_with_hosts("shared-host", host, host);
    assert_eq!(lookups_of(host), 1, "the shared host is looked up once");
    assert!(slow.len() <= 1, "steps at or above the bound: {slow:?}");
    assert_one_notice(host, "asset host");
}
