//! Task 4: `Client::from_shared` + socket-adopt (opcode 18). A `Client`
//! hands its live `ClientStream`/ISAAC cursor to another `Client`, and
//! `login(..., reconnect = true)` sends wrapper opcode **18** (server
//! response **15** keeps state) so a channel-head tune swaps net+sim
//! without a TCP drop. `from_shared` constructs over a shared `Arc<Cache>`
//! + iface template without a second unpack or `/crc` probe. The /tmp
//! cache has no packs, so `Client::new` never touches the network beyond
//! the listener planted here.
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use client::client::{Client, ClientConfig};
use client::config::{Cache, IfType, IfTypeMut};

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

/// One 274 login handshake over `s`: probe 14 + login-server byte, 8 probe
/// bytes, response 0, 8-byte seed, then assert the wrapper opcode (`16`
/// cold / `18` reconnect) and grant with `grant` (response 2 + staff +
/// mouse, or response 15 alone).
fn serve_login(s: &mut std::net::TcpStream, opcode: u8, grant: &[u8]) {
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).unwrap();
    assert_eq!(hdr[0], 14, "login server probe");
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    s.write_all(&[0]).unwrap(); // response 0 → send seed
    s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).unwrap();
    assert!(n > 0);
    assert_eq!(buf[0], opcode, "login wrapper opcode");
    s.write_all(grant).unwrap();
}

/// Host construct: one `Arc<Cache>`, the shared iface decode `Arc` and a
/// per-client mut-overlay template for every client — no `load_cache`
/// unpack, no `/crc` probe, `error_loading` false.
#[test]
fn from_shared_reuses_one_arc_without_unpack() {
    let tables = Arc::new(Cache::default());
    let template = Arc::new(vec![None, Some(Box::new(IfType::default()))]);
    let mut_template = vec![None, Some(Box::new(IfTypeMut::default()))];
    let mut a = Client::from_shared(cfg(), Arc::clone(&tables), Arc::clone(&template), mut_template.clone());
    let b = Client::from_shared(cfg(), Arc::clone(&tables), Arc::clone(&template), mut_template);
    assert!(Arc::ptr_eq(&a.cache, &b.cache));
    assert!(Arc::ptr_eq(&a.cache, &tables));
    // One decode table for every client: both clients point at the same
    // `Arc`, and the template Arc is the only strong owner of the decode.
    assert!(Arc::ptr_eq(&a.ifaces, &b.ifaces));
    assert!(Arc::ptr_eq(&a.ifaces, &template));
    assert!(!a.error_loading);
    assert!(!b.error_loading);
    assert!(!a.draw, "host construct starts unheaded");
    assert!(
        !a.world.overlay_mesh,
        "unheaded construct must not default overlay_mesh on (World::new does for unit tests)"
    );
    // hide stays per-client: a's write goes to a's dense `IfTypeMut`
    // overlay, so b's slot 1 must not reflect it (the shared decode and
    // the overlay template are both untouched).
    a.iface_mut(1).unwrap().hide = true;
    assert!(a.if_(1).unwrap().hide);
    assert!(!b.if_(1).unwrap().hide);
}

/// Fifty slots share one IfTypeMut template Arc until a slot writes;
/// `iface_mut` copy-on-writes that slot's vec and leaves siblings on the
/// template.
#[test]
fn mut_overlay_template_is_arc_until_first_write() {
    let tables = Arc::new(Cache::default());
    let template = Arc::new(vec![None, Some(Box::new(IfType::default()))]);
    let mut_template = Arc::new(vec![None, Some(Box::new(IfTypeMut::default()))]);
    let mut a = Client::from_shared(
        cfg(),
        Arc::clone(&tables),
        Arc::clone(&template),
        Arc::clone(&mut_template),
    );
    let b = Client::from_shared(
        cfg(),
        Arc::clone(&tables),
        Arc::clone(&template),
        Arc::clone(&mut_template),
    );
    assert!(
        Arc::ptr_eq(&a.ifaces_mut, &mut_template),
        "from_shared must clone the Arc, not the vec"
    );
    assert!(Arc::ptr_eq(&a.ifaces_mut, &b.ifaces_mut));
    a.iface_mut(1).unwrap().hide = true;
    assert!(
        !Arc::ptr_eq(&a.ifaces_mut, &mut_template),
        "first write must COW A's overlay"
    );
    assert!(
        Arc::ptr_eq(&b.ifaces_mut, &mut_template),
        "sibling must stay on the template"
    );
    assert!(a.if_(1).unwrap().hide);
    assert!(!b.if_(1).unwrap().hide);
}

/// The per-client mut overlay is a small dense struct, not a decode copy:
/// writing hide and an inv slot must never clone the decode's scripts/
/// children/strings onto the overlay (the `Arc` stays the same object and
/// `IfTypeMut` has no room for them).
#[test]
fn mut_overlay_is_small_and_decode_stays_ptr_eq_after_writes() {
    use std::sync::Arc;
    assert!(
        std::mem::size_of::<IfTypeMut>() * 3 < std::mem::size_of::<IfType>(),
        "the mut overlay must stay far smaller than the decode"
    );
    let tables = Arc::new(Cache::default());
    let template = Arc::new(vec![None, Some(Box::new(IfType {
        id: 1,
        r#type: 0,
        scripts: Some(vec![vec![1, 2, 3]]),
        children: Some(vec![2]),
        ..IfType::default()
    }))]);
    let mut_template = vec![None, Some(Box::new(IfTypeMut::default()))];
    let mut a = Client::from_shared(cfg(), Arc::clone(&tables), Arc::clone(&template), mut_template.clone());
    let b = Client::from_shared(cfg(), Arc::clone(&tables), Arc::clone(&template), mut_template);
    // mutate hide + one inv slot
    {
        let m = a.iface_mut(1).unwrap();
        m.hide = true;
        m.link_obj_type = Some(vec![1, 0]);
        m.link_obj_number = Some(vec![7, 0]);
    }
    // the shared decode is untouched: same Arc, same component, and b sees
    // the decoded hide/scripts (not a's writes).
    assert!(Arc::ptr_eq(&a.ifaces, &b.ifaces));
    assert!(Arc::ptr_eq(&a.ifaces, &template));
    assert_eq!(a.if_(1).unwrap().scripts.as_ref().unwrap()[0], vec![1, 2, 3]);
    assert!(a.if_(1).unwrap().hide);
    assert!(!b.if_(1).unwrap().hide);
    assert_eq!(b.if_(1).unwrap().link_obj_type, &None);
}

/// The channel-head tune: `head` logs in cold, hands its live socket +
/// ISAAC cursors to a second `Client` built `from_shared`, and that client
/// reconnects wrapper opcode **18** over the **same** TCP (response **15**
/// keeps state). The server sees exactly one connection.
#[test]
fn adopt_reconnects_opcode_18_without_tcp_drop() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let server = {
        let accepted = Arc::clone(&accepted);
        thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            accepted.fetch_add(1, Ordering::SeqCst);
            // cold login (wrapper 16 → response 2)
            serve_login(&mut s, 16, &[2, 0, 0]);
            // the reconnect must ride the SAME socket (wrapper 18 →
            // response 15); a fresh TCP instead of the adopted socket
            // would land in the backlog watch below.
            serve_login(&mut s, 18, &[15]);
            listener.set_nonblocking(true).unwrap();
            for _ in 0..200 {
                match listener.accept() {
                    Ok(_) => {
                        accepted.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            }
        })
    };

    let mut head = Client::new(cfg());
    head.config.host = addr.ip().to_string();
    head.config.port = addr.port();
    head.login("bob", "pw", false).unwrap();
    assert!(head.ingame);
    assert!(head.stream.is_some());

    // the tune: a fresh sim `Client` over the shared cache adopts the
    // live stream + ISAAC cursors; the source must not touch the socket.
    let mut sim = Client::from_shared(cfg(), Arc::new(Cache::default()), Arc::new(vec![]), vec![]);
    sim.config.host = addr.ip().to_string();
    sim.config.port = addr.port();
    assert!(sim.adopt_from(&mut head).is_some());
    assert!(head.stream.is_none(), "handoff must detach the source");
    assert!(head.random_in.is_none());
    assert!(sim.stream.is_some(), "adopted socket must be live");
    assert!(sim.random_in.is_some(), "inbound ISAAC cursor must move");
    assert!(sim.out.random.is_some(), "outbound ISAAC cursor must move");
    assert!(sim.baton, "adopt arms the in-place opcode-18 reconnect");

    // response 15 keeps state: the marker must survive the reconnect (a
    // response-2 cold login would wipe it).
    sim.player_count = 7;
    sim.login("bob", "pw", true).unwrap();
    assert!(sim.ingame);
    assert_eq!(sim.last_login_reconnect, Some(true));
    assert!(sim.stream.is_some());
    assert_eq!(sim.player_count, 7, "response 15 must keep client state");
    assert!(!sim.baton, "baton is consumed by the login it arms");
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the reconnect must reuse the adopted TCP, not open a fresh one"
    );
    server.join().unwrap();
}

/// Adopting a `Client` with no live stream is a no-op (`None`) and does
/// not arm `baton`.
#[test]
fn adopt_without_live_stream_returns_none() {
    let mut a = Client::new(cfg());
    let mut b = Client::new(cfg());
    assert!(b.adopt_from(&mut a).is_none());
    assert!(!b.baton);
    assert!(b.stream.is_none());
}
