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

/// One of the eleven `ClientGens` families, in struct field order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GensField {
    Npc,
    Player,
    Inv,
    Varp,
    Stat,
    Chat,
    Scene,
    Iface,
    Camera,
    MapFlag,
    World,
}

impl GensField {
    const ALL: [GensField; 11] = [
        GensField::Npc,
        GensField::Player,
        GensField::Inv,
        GensField::Varp,
        GensField::Stat,
        GensField::Chat,
        GensField::Scene,
        GensField::Iface,
        GensField::Camera,
        GensField::MapFlag,
        GensField::World,
    ];

    fn get(self, g: &ClientGens) -> u64 {
        match self {
            GensField::Npc => g.npc,
            GensField::Player => g.player,
            GensField::Inv => g.inv,
            GensField::Varp => g.varp,
            GensField::Stat => g.stat,
            GensField::Chat => g.chat,
            GensField::Scene => g.scene,
            GensField::Iface => g.iface,
            GensField::Camera => g.camera,
            GensField::MapFlag => g.map_flag,
            GensField::World => g.world,
        }
    }
}

/// Fresh client, apply `ptype`, then assert exactly `family` is 1 and every
/// other family is 0 (`Client::new` leaves all gens zero).
fn check_bump(ptype: i32, family: GensField) {
    let mut c = Client::new(cfg());
    c.bump_gens(ptype);
    for f in GensField::ALL {
        let expected = if f == family { 1 } else { 0 };
        assert_eq!(f.get(&c.gens), expected, "bump_gens({ptype}) family {f:?}");
    }
}

/// Pins the exact server-packet → generation-family mapping in `bump_gens`
/// (client.rs). Every `ServerProt` const in a `bump_gens` arm must bump
/// exactly its family by 1 and no other; `REBUILD_NORMAL` bumps all 11.
/// A future edit to a packet or a family that drifts this mapping fails here.
#[test]
fn packet_to_family_mapping_is_exact() {
    let cases: &[(i32, GensField)] = &[
        // npc
        (ServerProt::NPC_INFO, GensField::Npc),
        // player
        (ServerProt::PLAYER_INFO, GensField::Player),
        // inv
        (ServerProt::UPDATE_INV_FULL, GensField::Inv),
        (ServerProt::UPDATE_INV_PARTIAL, GensField::Inv),
        (ServerProt::UPDATE_INV_STOP_TRANSMIT, GensField::Inv),
        // varp
        (ServerProt::VARP_SMALL, GensField::Varp),
        (ServerProt::VARP_LARGE, GensField::Varp),
        (ServerProt::VARP_SYNC, GensField::Varp),
        // stat
        (ServerProt::UPDATE_STAT, GensField::Stat),
        (ServerProt::UPDATE_RUNENERGY, GensField::Stat),
        (ServerProt::UPDATE_RUNWEIGHT, GensField::Stat),
        // chat
        (ServerProt::MESSAGE_GAME, GensField::Chat),
        (ServerProt::MESSAGE_PRIVATE, GensField::Chat),
        // scene
        (ServerProt::UPDATE_ZONE_PARTIAL_FOLLOWS, GensField::Scene),
        (ServerProt::UPDATE_ZONE_FULL_FOLLOWS, GensField::Scene),
        (ServerProt::UPDATE_ZONE_PARTIAL_ENCLOSED, GensField::Scene),
        (ServerProt::P_LOCMERGE, GensField::Scene),
        (ServerProt::LOC_ANIM, GensField::Scene),
        (ServerProt::OBJ_DEL, GensField::Scene),
        (ServerProt::OBJ_REVEAL, GensField::Scene),
        (ServerProt::LOC_ADD_CHANGE, GensField::Scene),
        (ServerProt::MAP_PROJANIM, GensField::Scene),
        (ServerProt::LOC_DEL, GensField::Scene),
        (ServerProt::OBJ_COUNT, GensField::Scene),
        (ServerProt::MAP_ANIM, GensField::Scene),
        (ServerProt::OBJ_ADD, GensField::Scene),
        // iface
        (ServerProt::IF_OPENCHAT, GensField::Iface),
        (ServerProt::IF_OPENMAIN_SIDE, GensField::Iface),
        (ServerProt::IF_CLOSE, GensField::Iface),
        (ServerProt::IF_SETICON, GensField::Iface),
        (ServerProt::IF_SHOWICON, GensField::Iface),
        (ServerProt::IF_OPENMAIN, GensField::Iface),
        (ServerProt::IF_OPENSIDE, GensField::Iface),
        (ServerProt::IF_OPENOVERLAY, GensField::Iface),
        (ServerProt::IF_SETCOLOUR, GensField::Iface),
        (ServerProt::IF_SETHIDE, GensField::Iface),
        (ServerProt::IF_SETOBJECT, GensField::Iface),
        (ServerProt::IF_SETMODEL, GensField::Iface),
        (ServerProt::IF_SETANIM, GensField::Iface),
        (ServerProt::IF_SETPLAYERHEAD, GensField::Iface),
        (ServerProt::IF_SETTEXT, GensField::Iface),
        (ServerProt::IF_SETNPCHEAD, GensField::Iface),
        (ServerProt::IF_SETPOSITION, GensField::Iface),
        (ServerProt::IF_SETSCROLLPOS, GensField::Iface),
        (ServerProt::P_COUNTDIALOG, GensField::Iface),
        // camera
        (ServerProt::CAM_LOOKAT, GensField::Camera),
        (ServerProt::CAM_SHAKE, GensField::Camera),
        (ServerProt::CAM_MOVETO, GensField::Camera),
        (ServerProt::CAM_RESET, GensField::Camera),
        // map_flag
        (ServerProt::UNSET_MAP_FLAG, GensField::MapFlag),
        // world
        (ServerProt::SET_MULTIWAY, GensField::World),
    ];

    for &(ptype, family) in cases {
        check_bump(ptype, family);
    }

    // REBUILD_NORMAL swaps the whole scene: every family advances by 1.
    let mut c = Client::new(cfg());
    c.bump_gens(ServerProt::REBUILD_NORMAL);
    for f in GensField::ALL {
        assert_eq!(f.get(&c.gens), 1, "REBUILD_NORMAL missed family {f:?}");
    }
}
