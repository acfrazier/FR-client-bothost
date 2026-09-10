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

/// Pack MSB-first bit fields the same way `Packet::gbit` consumes them.
fn pack_bits(fields: &[(u32, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u64 = 0;
    let mut nbits: usize = 0;
    for &(val, width) in fields {
        assert!(width <= 32);
        acc = (acc << width) | (u64::from(val) & ((1u64 << width) - 1));
        nbits += width;
        while nbits >= 8 {
            let shift = nbits - 8;
            out.push(((acc >> shift) & 0xff) as u8);
            nbits -= 8;
            acc &= if nbits == 0 { 0 } else { (1u64 << nbits) - 1 };
        }
    }
    if nbits > 0 {
        out.push(((acc << (8 - nbits)) & 0xff) as u8);
    }
    out
}

fn default_appearance_block(gender: u8, combat: u8) -> Vec<u8> {
    // gender, headicons, 12 zero parts, 5 zero colours, 7 anims as 0xFFFF,
    // empty name (8 zero bytes), combat, skill 0.
    let mut appearance = vec![0u8; 44];
    appearance[0] = gender;
    for b in appearance.iter_mut().skip(19).take(14) {
        *b = 0xff;
    }
    appearance[41] = combat;
    appearance
}

/// New-vis entry for `index` relative to the local player, optional extended.
fn new_vis_bits(index: u32, extended: bool) -> Vec<(u32, usize)> {
    vec![
        (index, 11),
        (0, 5), // dx
        (0, 5), // dz
        (0, 1), // jump
        (u32::from(extended), 1),
    ]
}

/// `PLAYER_INFO` that introduces remote indices via new-vis with APPEARANCE
/// extended blocks, then a later remove + re-entry without a new appearance
/// must re-apply the cached packet (cursor reset) with independent slot data.
#[test]
fn player_info_cached_appearance_reapplied_after_remove_reentry() {
    let mut c = client();
    c.local_player = Some(ClientPlayer::at(10, 10));

    let app5 = default_appearance_block(0, 40);
    let app7 = default_appearance_block(1, 77);

    // Frame 1: no local update, empty old-vis, new-vis players 5 and 7 with
    // extended, sentinel 2047, then two APPEARANCE (0x01) blocks.
    let mut bits = vec![(0u32, 1), (0u32, 8)]; // local info=0, old-vis count=0
    bits.extend(new_vis_bits(5, true));
    bits.extend(new_vis_bits(7, true));
    bits.push((2047, 11));
    let mut frame1 = pack_bits(&bits);
    frame1.push(0x01); // APPEARANCE for player 5
    frame1.push(app5.len() as u8);
    frame1.extend_from_slice(&app5);
    frame1.push(0x01); // APPEARANCE for player 7
    frame1.push(app7.len() as u8);
    frame1.extend_from_slice(&app7);

    c.psize = frame1.len() as i32;
    let mut p = Packet::new(frame1);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);

    assert!(c.ingame, "first frame must consume exactly");
    assert_eq!(c.player_count, 2);
    let p5 = c.players[5].as_ref().expect("player 5 present");
    let p7 = c.players[7].as_ref().expect("player 7 present");
    assert!(p5.is_ready(), "player 5 appearance must mark ready");
    assert!(p7.is_ready(), "player 7 appearance must mark ready");
    assert_eq!(p5.gender, 0);
    assert_eq!(p7.gender, 1);
    assert_eq!(p5.combat_level, 40);
    assert_eq!(p7.combat_level, 77);
    assert!(
        c.player_appearance_buffer[5].is_some(),
        "appearance cached for slot 5"
    );
    assert!(
        c.player_appearance_buffer[7].is_some(),
        "appearance cached for slot 7"
    );
    // Leave cursor at end after first apply so re-entry must reset pos.
    if let Some(buf) = c.player_appearance_buffer[5].as_mut() {
        buf.pos = buf.length();
    }
    if let Some(buf) = c.player_appearance_buffer[7].as_mut() {
        buf.pos = buf.length();
    }
    // Distinct boxed slots must not alias: mutating one packet body must not
    // change the other slot's stored bytes.
    let len5 = c.player_appearance_buffer[5].as_ref().unwrap().length();
    c.player_appearance_buffer[5].as_mut().unwrap().data_mut()[18] = 0xAB; // fifth colour byte after 12 zero parts
    assert_ne!(
        c.player_appearance_buffer[7].as_ref().unwrap().data()[18],
        0xAB,
        "slot 7 must not share slot 5 packet storage"
    );
    // Restore gender/colour integrity for re-apply assertions (combat/gender).
    c.player_appearance_buffer[5].as_mut().unwrap().data_mut()[18] = 0;
    assert_eq!(
        c.player_appearance_buffer[5].as_ref().unwrap().length(),
        len5
    );

    // Frame 2: advance loop_cycle so removal drops slots whose cycle is stale.
    c.loop_cycle += 1;
    // local info=0, old-vis count=0 → both prior player_ids go to removal.
    let frame2 = pack_bits(&[(0u32, 1), (0u32, 8), (2047u32, 11)]);
    c.psize = frame2.len() as i32;
    let mut p = Packet::new(frame2);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);
    assert!(c.ingame, "removal frame must consume exactly");
    assert_eq!(c.player_count, 0);
    assert!(c.players[5].is_none(), "player 5 removed");
    assert!(c.players[7].is_none(), "player 7 removed");
    assert!(
        c.player_appearance_buffer[5].is_some() && c.player_appearance_buffer[7].is_some(),
        "cached appearance packets survive player removal"
    );

    // Frame 3: re-enter 5 and 7 without extended appearance; cache re-applies.
    let mut bits3 = vec![(0u32, 1), (0u32, 8)];
    bits3.extend(new_vis_bits(5, false));
    bits3.extend(new_vis_bits(7, false));
    bits3.push((2047, 11));
    let frame3 = pack_bits(&bits3);
    c.loop_cycle += 1;
    c.psize = frame3.len() as i32;
    let mut p = Packet::new(frame3);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);

    assert!(c.ingame, "re-entry frame must consume exactly");
    assert_eq!(c.player_count, 2);
    let p5 = c.players[5].as_ref().expect("player 5 re-entered");
    let p7 = c.players[7].as_ref().expect("player 7 re-entered");
    assert!(
        p5.is_ready(),
        "cached appearance must re-apply on player 5 re-entry"
    );
    assert!(
        p7.is_ready(),
        "cached appearance must re-apply on player 7 re-entry"
    );
    assert_eq!(p5.gender, 0, "slot 5 keeps its cached gender");
    assert_eq!(p7.gender, 1, "slot 7 keeps its cached gender");
    assert_eq!(p5.combat_level, 40, "slot 5 keeps its cached combat");
    assert_eq!(p7.combat_level, 77, "slot 7 keeps its cached combat");
    // Cursor restored into the buffer after re-apply path (set_appearance ends
    // past the block; buffer entry remains independently owned).
    assert!(c.player_appearance_buffer[5].is_some());
    assert!(c.player_appearance_buffer[7].is_some());
    let pos5 = c.player_appearance_buffer[5].as_ref().unwrap().pos;
    let pos7 = c.player_appearance_buffer[7].as_ref().unwrap().pos;
    assert!(
        pos5 > 0 && pos7 > 0,
        "re-apply must have read the cached packets (pos advanced)"
    );
}
