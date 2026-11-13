use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision, MiniMenuAction};
use client::io::Packet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn relog(revision: ClientRevision, expected: &[u8]) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = [0; 2];
            stream.read_exact(&mut header).unwrap();
            stream.write_all(&[0; 17]).unwrap();
            stream.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 16);
            let mut login = vec![0; header[1] as usize];
            stream.read_exact(&mut login).unwrap();
            stream.write_all(&[2, 0, 0]).unwrap();
        }
    });
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: address.ip().to_string(),
            port: address.port(),
            cache_dir: std::env::temp_dir()
                .join(format!("fidelity-b7-{}-{revision:?}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            members: true,
            lowmem: true,
        },
        revision,
    );
    c.login("fixture", "fixture", false).unwrap();
    c.in_multizone = 1;
    c.oplogic1 = 139;
    c.oplogic2 = 124;
    c.oplogic3 = 100;
    c.oplogic4 = 52;
    c.oplogic5 = 66;
    c.oplogic6 = 100;
    c.oplogic7 = 100;
    c.oplogic8 = 100;
    c.oplogic9 = 100;
    c.logout();
    c.login("fixture", "fixture", false).unwrap();
    server.join().unwrap();
    let counters = [
        c.oplogic1, c.oplogic2, c.oplogic3, c.oplogic4, c.oplogic5, c.oplogic6, c.oplogic7,
        c.oplogic8, c.oplogic9,
    ];
    c.stream = None;
    c.out = Packet::alloc(1);
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.players[3] = Some(Box::new(ClientPlayer::at(10, 10)));
    c.menu_action[0] = MiniMenuAction::OP_PLAYER1;
    c.menu_param_a[0] = 3;
    c.doAction(0);
    // Java 289 client.java:8494-8502 resets the counters before this action.
    assert_eq!(&c.out.data()[..c.out.pos], expected);
    assert_eq!(counters, [0; 9]);
    assert_eq!(c.in_multizone, 0);
    println!("Client inline bytes: {}", std::mem::size_of::<Client>());
}

#[test]
fn fidelity_b7_relog_274() {
    relog(ClientRevision::R274, &[138, 5, 0, 0, 10, 0, 10, 109, 0, 3]);
}
#[test]
fn fidelity_b7_relog_289() {
    relog(ClientRevision::R289, &[67, 5, 0, 0, 10, 0, 10, 220, 0, 3]);
}
#[test]
fn fidelity_b7_relog_377() {
    relog(
        ClientRevision::R377,
        &[247, 5, 138, 0, 0, 138, 0, 245, 131, 0],
    );
}

#[test]
fn fidelity_b7_reconnect_keeps_multiway_and_menu_counters() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = [0; 2];
            stream.read_exact(&mut header).unwrap();
            stream.write_all(&[0; 17]).unwrap();
            stream.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 18);
            let mut login = vec![0; header[1] as usize];
            stream.read_exact(&mut login).unwrap();
            stream.write_all(&[15]).unwrap();
        });
        let mut c = Client::new_with_revision(
            ClientConfig {
                host: address.ip().to_string(),
                port: address.port(),
                cache_dir: std::env::temp_dir()
                    .join(format!("fidelity-b7-reconnect-{}", std::process::id()))
                    .to_string_lossy()
                    .into_owned(),
                members: true,
                lowmem: true,
            },
            revision,
        );
        c.in_multizone = 1;
        c.oplogic1 = 1;
        c.oplogic2 = 2;
        c.oplogic3 = 3;
        c.oplogic4 = 4;
        c.oplogic5 = 5;
        c.oplogic6 = 6;
        c.oplogic7 = 7;
        c.oplogic8 = 8;
        c.oplogic9 = 9;
        c.login("fixture", "fixture", true).unwrap();
        server.join().unwrap();
        assert_eq!(c.in_multizone, 1);
        assert_eq!(
            [
                c.oplogic1, c.oplogic2, c.oplogic3, c.oplogic4, c.oplogic5, c.oplogic6, c.oplogic7,
                c.oplogic8, c.oplogic9,
            ],
            [1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
    }
}
