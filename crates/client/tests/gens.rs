// Packet-family generation tests (Task 2): `ClientGens` counters tell the
// host which world slices changed since its last poll. `bump_gens` maps a
// `ServerProt` opcode to its family; `handle_packet` calls it after every
// applied packet. The /tmp cache has no packs, so `Client::new` falls back
// to `Cache::default()` and never touches the network.

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

#[test]
fn npc_info_bumps_npc_gen_only() {
    let mut c = Client::new(cfg());
    let before = c.gens;
    c.bump_gens(ServerProt::NPC_INFO);
    assert_eq!(c.gens.npc, before.npc + 1);
    assert_eq!(c.gens.player, before.player);
    assert_eq!(c.gens.inv, before.inv);
}

#[test]
fn player_info_bumps_player_gen() {
    let mut c = Client::new(cfg());
    c.bump_gens(ServerProt::PLAYER_INFO);
    assert_eq!(c.gens.player, 1);
}

#[test]
fn rebuild_bumps_all_gens() {
    let mut c = Client::new(cfg());
    c.bump_gens(ServerProt::REBUILD_NORMAL);
    assert!(c.gens.npc >= 1 && c.gens.player >= 1 && c.gens.inv >= 1);
}

/// `handle_packet` bumps the family after a successful dispatch: a
/// `VARP_SMALL` frame lands in the varp generation and nothing else.
#[test]
fn handle_packet_bumps_varp_gen() {
    let mut c = Client::new(cfg());
    let mut p = Packet::alloc(0);
    p.p2(1);
    p.p1(7);
    p.pos = 0;
    c.handle_packet(ServerProt::VARP_SMALL, &mut p);
    assert_eq!(c.var[1], 7);
    assert_eq!(c.gens.varp, 1);
    assert_eq!(c.gens.inv, 0);
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

/// A panicking dispatch is Java `catch (Exception)` → T2 + logout; the
/// half-applied frame bumps every family so the host re-reads all slices.
#[test]
fn t2_logout_bumps_all_gens() {
    let mut c = Client::new(cfg());
    c.local_player = Some(ClientPlayer::at(1, 1));
    c.psize = 1;
    let mut p = Packet::new(vec![0xe0]);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);
    assert!(!c.ingame);
    assert_eq!(c.gens.npc, 1);
    assert_eq!(c.gens.player, 1);
    assert_eq!(c.gens.scene, 1);
}

/// The generation bookkeeping derives the Java-visible shape the host reads:
/// `Default` (zeroed), copyable, and every counter is a plain `u64`.
#[test]
fn gens_defaults_to_zero() {
    let c = Client::new(cfg());
    let g: ClientGens = Default::default();
    assert_eq!(c.gens.npc, g.npc);
    assert_eq!(c.gens.scene, g.scene);
}
