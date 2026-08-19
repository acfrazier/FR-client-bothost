//! ServerProt handler tests (Task 17): `handle_packet` dispatch for the
//! `Client.ts` `ptype` switch handlers. Packets are built by hand and passed
//! straight to `handle_packet`, skipping the socket and Isaac.

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

#[test]
fn varp_small_writes_var() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p2(1);
    p.p1(7);
    p.pos = 0;
    c.handle_packet(ServerProt::VARP_SMALL, &mut p);
    assert_eq!(c.var[1], 7);
}

#[test]
fn varp_small_value_is_signed() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p2(0);
    p.p1(0xff);
    p.pos = 0;
    c.handle_packet(ServerProt::VARP_SMALL, &mut p);
    assert_eq!(c.var[0], -1);
}

#[test]
fn varp_large_writes_var() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p2(2);
    p.p4(999);
    p.pos = 0;
    c.handle_packet(ServerProt::VARP_LARGE, &mut p);
    assert_eq!(c.var[2], 999);
}

#[test]
fn varp_sync_copies_served_vars() {
    let mut c = client();
    c.var = vec![5];
    c.var_serv = vec![7];
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::VARP_SYNC, &mut p);
    assert_eq!(c.var[0], 7);
}

#[test]
fn logout_clears_ingame() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::LOGOUT, &mut p);
    assert!(!c.ingame);
}

/// The dedicated LOGOUT arm resets `ptype` like every handled packet; the
/// unknown-opcode default does not, so this pins the branch.
#[test]
fn logout_resets_ptype() {
    let mut c = client();
    c.ptype = 7;
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::LOGOUT, &mut p);
    assert_eq!(c.ptype, -1);
}

#[test]
fn midi_song_sets_next() {
    let mut c = client();
    c.midi_active = true;
    let mut p = Packet::alloc(0);
    p.p2(3);
    p.pos = 0;
    c.handle_packet(ServerProt::MIDI_SONG, &mut p);
    assert_eq!(c.next_midi_song, 3);
}

#[test]
fn midi_song_65535_is_no_song() {
    let mut c = client();
    c.midi_active = true;
    let mut p = Packet::alloc(0);
    p.p2(65535);
    p.pos = 0;
    c.handle_packet(ServerProt::MIDI_SONG, &mut p);
    assert_eq!(c.next_midi_song, -1);
}

#[test]
fn midi_jingle_sets_song_and_delay() {
    let mut c = client();
    c.midi_active = true;
    let mut p = Packet::alloc(0);
    p.p2(5);
    p.p2(10);
    p.pos = 0;
    c.handle_packet(ServerProt::MIDI_JINGLE, &mut p);
    assert_eq!(c.midi_song, 5);
    assert_eq!(c.next_music_delay, 10);
}

#[test]
fn update_stat_sets_xp_and_levels() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p1(0);
    p.p4(1234);
    p.p1(50);
    p.pos = 0;
    c.handle_packet(ServerProt::UPDATE_STAT, &mut p);
    assert_eq!(c.stat_xp[0], 1234);
    assert_eq!(c.stat_effective_level[0], 50);
    assert!(c.stat_base_level[0] > 1);
}

#[test]
fn update_runenergy_sets_energy() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p1(77);
    p.pos = 0;
    c.handle_packet(ServerProt::UPDATE_RUNENERGY, &mut p);
    assert_eq!(c.runenergy, 77);
}

#[test]
fn reset_anims_clears_primary_anims() {
    let mut c = client();
    c.players[1] = Some(ClientPlayer::at(0, 0));
    c.players[1].as_mut().unwrap().primary_anim = 7;
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::RESET_ANIMS, &mut p);
    assert_eq!(c.players[1].as_ref().unwrap().primary_anim, -1);
}

#[test]
fn unset_map_flag_clears_flag() {
    let mut c = client();
    c.minimap_flag_x = 5;
    c.minimap_flag_z = 6;
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::UNSET_MAP_FLAG, &mut p);
    assert_eq!(c.minimap_flag_x, 0);
    assert_eq!(c.minimap_flag_z, 6);
}

#[test]
fn unknown_ptype_logs_out() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    c.handle_packet(0, &mut p);
    assert!(!c.ingame);
}

/// A `PLAYER_INFO` frame that teleports the local player to tile (5, 6):
/// local block op 3 (2-bit minusedlevel 0, 7-bit x 5, 7-bit z 6, jump 0,
/// extended 0) then an empty old-visibility list (8-bit count 0). Bytes are
/// the 29-bit stream MSB-first: E0 50 C0 00.
#[test]
fn player_info_teleports_local_player() {
    let mut c = client();
    c.local_player = Some(ClientPlayer::at(1, 1));
    c.psize = 4;
    let mut p = Packet::alloc(0);
    p.p1(0xe0);
    p.p1(0x50);
    p.p1(0xc0);
    p.p1(0x00);
    p.pos = 0;
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);
    assert!(c.ingame); // frame consumed exactly; no T2 logout
    let local = c.local_player.unwrap();
    assert_eq!(local.route_x[0], 5);
    assert_eq!(local.route_z[0], 6);
}

/// An `NPC_INFO` frame with an empty old-visibility list (8-bit count 0) is
/// consumed without a size mismatch.
#[test]
fn npc_info_empty_list_is_ok() {
    let mut c = client();
    c.psize = 1;
    let mut p = Packet::alloc(0);
    p.p1(0x00);
    p.pos = 0;
    c.handle_packet(ServerProt::NPC_INFO, &mut p);
    assert_eq!(c.npc_count, 0);
    assert!(c.ingame);
}
