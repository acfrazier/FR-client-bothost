//! `ClientGens` generation tests: per-family monotonic counters the host
//! reads to know which world slices changed since its last poll.
//! `handle_packet` bumps one family per applied packet; `REBUILD_NORMAL`
//! and `LOGOUT` (and T2 aborts) bump every family. The /tmp cache has no
//! packs, so `Client::new` falls back to `Cache::default()` and never
//! touches the network.

use client::client::{Client, ClientConfig, ClientPlayer, ClientGens};
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
}

/// `REBUILD_NORMAL` swaps the whole scene, so every family is stale.
#[test]
fn rebuild_bumps_all_gens() {
    let mut c = Client::new(cfg());
    c.bump_gens(ServerProt::REBUILD_NORMAL);
    assert!(c.gens.npc >= 1 && c.gens.player >= 1 && c.gens.inv >= 1);
    assert!(c.gens.varp >= 1 && c.gens.stat >= 1 && c.gens.chat >= 1 && c.gens.scene >= 1);
}

/// `LOGOUT` resets the whole world, so the `handle_packet` path bumps every
/// family generation.
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
}

/// The generation bookkeeping derives the Java-visible shape the host reads:
/// `Default` (zeroed) and every counter a plain `u64`.
#[test]
fn gens_defaults_to_zero() {
    let c = Client::new(cfg());
    let g: ClientGens = Default::default();
    assert_eq!(c.gens.npc, g.npc);
    assert_eq!(c.gens.scene, g.scene);
}
