// Task 7: PLAYER_INFO appearance lands on `local_player`. Java
// `localPlayer` IS `players[LOCAL_PLAYER_INDEX]` (the same object), so
// `getPlayerPosExtended`'s appearance mask reaches the drawn entity; the
// Rust login clones a default into both, so the mask writes must target
// `local_player` directly or `ready` never flips on the drawn player and
// `addPlayer(true)` skips the invisible body.
use client::client::{Client, ClientConfig, ClientPlayer};
use client::io::{Packet, ServerProt};

fn client() -> Client {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c
}

/// A `PLAYER_INFO` frame that sends an appearance + face_entity block
/// (mask 0x05) for the local player (index 2047): local block op 0, an
/// empty old-vis list, the 2047 new-vis sentinel, then the extended entry.
/// The 44-byte appearance block is all defaults (gender 0, no parts, no
/// colours, -1 anims, empty name), which marks `ready`.
#[test]
fn player_info_appearance_lands_on_local_player() {
    let mut c = client();
    let mut slot = ClientPlayer::default();
    slot.entity.x = 3 * 128 + 64;
    slot.entity.z = 4 * 128 + 64;
    slot.entity.route_x[0] = 3;
    slot.entity.route_z[0] = 4;
    c.players[2047] = Some(Box::new(slot));
    let mut local = ClientPlayer::at(10, 10);
    local.entity.x = 10 * 128 + 64;
    local.entity.z = 10 * 128 + 64;
    c.local_player = Some(local);

    // appearance data: gender, headicons, 12 zero parts, 5 zero colours,
    // 7 anims as 0xFFFF (-1), 8 zero name bytes, combat 0, skill 0.
    let mut appearance = vec![0u8; 44];
    for b in appearance.iter_mut().skip(19).take(14) {
        *b = 0xff;
    }

    // bits: info 1, op 00 (3 bits), old-vis count 0 (8 bits), 2047
    // sentinel (11 bits), then byte-aligned extended mask 0x05
    // (APPEARANCE | FACEENTITY) + length 44 + appearance + face_entity.
    let mut frame = vec![0x80, 0x1f, 0xfc, 0x05, 44];
    frame.extend_from_slice(&appearance);
    frame.extend_from_slice(&[0x12, 0x34]); // face_entity 4660
    c.psize = frame.len() as i32;
    let mut p = Packet::new(frame);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);

    assert!(c.ingame); // frame consumed exactly; no T2 logout
    let local = c.local_player.as_ref().unwrap();
    assert!(
        local.is_ready(),
        "appearance must mark the drawn local player ready"
    );
    // x/z/route must not be copied from the stale `players[2047]` slot.
    assert_eq!(local.x, 10 * 128 + 64, "local x must be untouched");
    assert_eq!(local.z, 10 * 128 + 64, "local z must be untouched");
    assert_eq!(local.route_x[0], 10, "local route_x must be untouched");
    assert_eq!(local.route_z[0], 10, "local route_z must be untouched");
    assert_eq!(local.name.as_deref(), Some("Invalid Name"));
    assert_eq!(local.face_entity, 4660, "other masks land on local_player");
    let slot = c.players[2047].as_ref().unwrap();
    assert!(
        !slot.ready,
        "mask writes go to local_player, not the players[2047] clone"
    );
    assert_eq!(
        slot.face_entity, -1,
        "players[2047] clone keeps its own state"
    );
}
