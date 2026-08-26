//! `ClientGens` generation tests: per-family monotonic counters the host
//! reads to know which world slices changed since its last poll.
//! `handle_packet` bumps one family per applied packet; `REBUILD_NORMAL`
//! and `logout()` (T1/T2/LOGOUT) bump every family. The /tmp cache has no
//! packs, so `Client::new` falls back to `Cache::default()` and never
//! touches the network.

use client::client::{Client, ClientConfig, ClientGens, ClientPlayer};
use client::io::{Packet, ServerProt};

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

/// A `PLAYER_INFO` and an `UPDATE_INV_FULL` applied through `handle_packet`
/// each bump exactly their own family; the others stay 0.
#[test]
fn applied_packets_bump_their_family() {
    let mut c = Client::new(cfg());

    // PLAYER_INFO teleport frame (as server_packets.rs): E0 50 C0 00.
    c.ingame = true;
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

    // UPDATE_INV_FULL: com_id 0, no slots.
    let mut p = Packet::alloc(0);
    p.p2(0);
    p.p1(0);
    p.pos = 0;
    c.handle_packet(ServerProt::UPDATE_INV_FULL, &mut p);

    assert_eq!(c.gens.player, 1);
    assert_eq!(c.gens.inv, 1);
    assert_eq!(c.gens.npc, 0);
    assert_eq!(c.gens.varp, 0);
    assert_eq!(c.gens.stat, 0);
    assert_eq!(c.gens.chat, 0);
    assert_eq!(c.gens.scene, 0);
    assert_eq!(c.gens.iface, 0);
    assert_eq!(c.gens.camera, 0);
    assert_eq!(c.gens.map_flag, 0);
    assert_eq!(c.gens.world, 0);
}

/// `bump_gens` maps a `ServerProt` opcode to exactly one family.
#[test]
fn npc_info_bumps_npc_gen_only() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::NPC_INFO);
    assert_eq!(c.gens.npc, before.npc + 1);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
    assert_eq!(c.gens.varp, before.varp);
    assert_eq!(c.gens.stat, before.stat);
    assert_eq!(c.gens.chat, before.chat);
    assert_eq!(c.gens.scene, before.scene);
    assert_eq!(c.gens.iface, before.iface);
    assert_eq!(c.gens.camera, before.camera);
    assert_eq!(c.gens.map_flag, before.map_flag);
    assert_eq!(c.gens.world, before.world);
}

/// `bump_gens` maps interface mutation packets to the `iface` family.
#[test]
fn iface_packets_bump_iface_gen() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::IF_SETTEXT);
    assert_eq!(c.gens.iface, before.iface + 1);
    assert_eq!(c.gens.npc, before.npc);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
    assert_eq!(c.gens.varp, before.varp);
    assert_eq!(c.gens.stat, before.stat);
    assert_eq!(c.gens.chat, before.chat);
    assert_eq!(c.gens.scene, before.scene);
    assert_eq!(c.gens.camera, before.camera);
    assert_eq!(c.gens.map_flag, before.map_flag);
    assert_eq!(c.gens.world, before.world);
}

/// `bump_gens` maps camera control packets to the `camera` family.
#[test]
fn camera_packets_bump_camera_gen() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::CAM_LOOKAT);
    assert_eq!(c.gens.camera, before.camera + 1);
    assert_eq!(c.gens.npc, before.npc);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
    assert_eq!(c.gens.varp, before.varp);
    assert_eq!(c.gens.stat, before.stat);
    assert_eq!(c.gens.chat, before.chat);
    assert_eq!(c.gens.scene, before.scene);
    assert_eq!(c.gens.iface, before.iface);
    assert_eq!(c.gens.map_flag, before.map_flag);
    assert_eq!(c.gens.world, before.world);
}

/// `bump_gens` maps `UNSET_MAP_FLAG` to the `map_flag` family.
#[test]
fn map_flag_packets_bump_map_flag_gen() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::UNSET_MAP_FLAG);
    assert_eq!(c.gens.map_flag, before.map_flag + 1);
    assert_eq!(c.gens.npc, before.npc);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
    assert_eq!(c.gens.varp, before.varp);
    assert_eq!(c.gens.stat, before.stat);
    assert_eq!(c.gens.chat, before.chat);
    assert_eq!(c.gens.scene, before.scene);
    assert_eq!(c.gens.iface, before.iface);
    assert_eq!(c.gens.camera, before.camera);
    assert_eq!(c.gens.world, before.world);
}

/// `bump_gens` maps world-level flag packets (`SET_MULTIWAY`) to the `world` family.
#[test]
fn world_packets_bump_world_gen() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::SET_MULTIWAY);
    assert_eq!(c.gens.world, before.world + 1);
    assert_eq!(c.gens.npc, before.npc);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
    assert_eq!(c.gens.varp, before.varp);
    assert_eq!(c.gens.stat, before.stat);
    assert_eq!(c.gens.chat, before.chat);
    assert_eq!(c.gens.scene, before.scene);
    assert_eq!(c.gens.iface, before.iface);
    assert_eq!(c.gens.camera, before.camera);
    assert_eq!(c.gens.map_flag, before.map_flag);
}

/// `REBUILD_NORMAL` swaps the whole scene, so every family is stale.
#[test]
fn rebuild_bumps_all_gens() {
    let mut c = Client::new(cfg());
    c.bump_gens(ServerProt::REBUILD_NORMAL);
    assert!(c.gens.npc >= 1 && c.gens.player >= 1 && c.gens.inv >= 1);
    assert!(c.gens.varp >= 1 && c.gens.stat >= 1 && c.gens.chat >= 1 && c.gens.scene >= 1);
    assert!(c.gens.iface >= 1 && c.gens.camera >= 1 && c.gens.map_flag >= 1 && c.gens.world >= 1);
}

/// `LOGOUT` resets the whole world, so the `handle_packet` path bumps every
/// family generation (once — `logout()` is the bump, not a second `bump_gens`).
#[test]
fn logout_bumps_all_gens() {
    let mut c = Client::new(cfg());
    let mut p = Packet::alloc(0);
    c.handle_packet(ServerProt::LOGOUT, &mut p);
    assert_eq!(c.gens.npc, 1);
    assert_eq!(c.gens.player, 1);
    assert_eq!(c.gens.inv, 1);
    assert_eq!(c.gens.varp, 1);
    assert_eq!(c.gens.stat, 1);
    assert_eq!(c.gens.chat, 1);
    assert_eq!(c.gens.scene, 1);
    assert_eq!(c.gens.iface, 1);
    assert_eq!(c.gens.camera, 1);
    assert_eq!(c.gens.map_flag, 1);
    assert_eq!(c.gens.world, 1);
}

/// Direct `logout()` (lost_con, tcp_in T2, in-band PLAYER/NPC T2) also
/// invalidates every family, matching spec `REBUILD/logout → all`.
#[test]
fn logout_method_bumps_all_gens() {
    let mut c = Client::new(cfg());
    c.logout();
    assert_eq!(c.gens.npc, 1);
    assert_eq!(c.gens.player, 1);
    assert_eq!(c.gens.inv, 1);
    assert_eq!(c.gens.varp, 1);
    assert_eq!(c.gens.stat, 1);
    assert_eq!(c.gens.chat, 1);
    assert_eq!(c.gens.scene, 1);
    assert_eq!(c.gens.iface, 1);
    assert_eq!(c.gens.camera, 1);
    assert_eq!(c.gens.map_flag, 1);
    assert_eq!(c.gens.world, 1);
}

/// T1 unknown opcode logs out without a mapped `bump_gens` arm; `logout()`
/// still moves every family so the host snapshot is not left live.
#[test]
fn t1_unknown_opcode_bumps_all_gens() {
    let mut c = Client::new(cfg());
    let mut p = Packet::alloc(0);
    c.handle_packet(1, &mut p);
    assert!(!c.ingame);
    assert_eq!(c.gens.npc, 1);
    assert_eq!(c.gens.player, 1);
    assert_eq!(c.gens.inv, 1);
    assert_eq!(c.gens.varp, 1);
    assert_eq!(c.gens.stat, 1);
    assert_eq!(c.gens.chat, 1);
    assert_eq!(c.gens.scene, 1);
    assert_eq!(c.gens.iface, 1);
    assert_eq!(c.gens.camera, 1);
    assert_eq!(c.gens.map_flag, 1);
    assert_eq!(c.gens.world, 1);
}

/// The generation bookkeeping derives the Java-visible shape the host reads:
/// `Default` (zeroed) and every counter a plain `u64`.
#[test]
fn gens_defaults_to_zero() {
    let c = Client::new(cfg());
    let g: ClientGens = Default::default();
    assert_eq!(c.gens.npc, g.npc);
    assert_eq!(c.gens.scene, g.scene);
    assert_eq!(c.gens.iface, g.iface);
    assert_eq!(c.gens.camera, g.camera);
    assert_eq!(c.gens.map_flag, g.map_flag);
    assert_eq!(c.gens.world, g.world);
}
