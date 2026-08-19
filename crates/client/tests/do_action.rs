// doAction/tryMove encode: menu arrays → MOVE_GAMECLICK / OPNPC2 through
// Packet::p1_enc with the client's outbound Isaac.
use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer, MiniMenuAction};
use client::io::{ClientProt, Isaac};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

// Isaac seed [1,2,3,4] yields first next_int = -621246914 (pinned in tests/isaac.rs),
// so p1_enc(207) = (207 + -621246914) & 0xff = 13 and p1_enc(233) = 39.
#[test]
fn walk_menu_writes_move_gameclick() {
    let mut c = client();
    c.ingame = true;
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    c.menu_action[0] = MiniMenuAction::WALK;
    c.menu_param_a[0] = 0;
    c.menu_param_b[0] = 10; // local x
    c.menu_param_c[0] = 10; // local z
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.doAction(0);
    // first outbound opcode byte is p1_enc(MOVE_GAMECLICK)
    let enc = (ClientProt::MOVE_GAMECLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(enc, 13);
    assert_eq!(c.out.data()[0], enc);
    // full packet for (5,5) -> (10,10) on an all-open grid: straight diagonal,
    // so size 5 (2*1+3), ctrl 0, single absolute tile (10,10), no steps
    assert_eq!(&c.out.data()[..c.out.pos], &[13, 5, 0, 0, 10, 0, 10]);
    assert_eq!(c.out.pos, 7);
}

#[test]
fn op_npc2_writes_opnpc2() {
    let mut c = client();
    c.ingame = true;
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = 42; // npc index
    // the OP_NPC branches guard on the scene entity and walk to it first:
    // both stand on (5,5), so the walk is a zero-step MOVE_OPCLICK
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.npc[42] = Some(ClientNpc::at(5, 5));
    c.doAction(0);
    // MOVE_OPCLICK id 138: (138 + -621246914) & 0xff
    let walk_enc = (ClientProt::MOVE_OPCLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(walk_enc, 200);
    // size 5 (2*1+3), ctrl 0, single absolute tile (5,5), no steps
    assert_eq!(&c.out.data()[..7], &[walk_enc, 5, 0, 0, 5, 0, 5]);
    // then the action itself: OPNPC2 id 233 encoded with the *second* Isaac
    // value (the walk consumed the first): (233 + 1957022519) & 0xff
    let enc = (ClientProt::OPNPC2.id.wrapping_add(1957022519)) as u8;
    assert_eq!(enc, 32);
    assert_eq!(&c.out.data()[7..10], [enc, 0, 42]);
    assert_eq!(c.out.pos, 10);
}

#[test]
fn try_move_writes_move_gameclick() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    assert_eq!(&c.out.data()[..c.out.pos], &[13, 5, 0, 0, 10, 0, 10]);
}

#[test]
fn try_move_type_1_writes_minimapclick_size_plus_14() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 1));
    // MOVE_MINIMAPCLICK id 86: (86 + -621246914) & 0xff
    let enc = (ClientProt::MOVE_MINIMAPCLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(c.out.data()[0], enc);
    // size byte includes the extra 14 bytes of the minimap packet
    assert_eq!(c.out.data()[1], 1 + 1 + 3 + 14);
}

#[test]
fn try_move_out_of_area_destination_returns_false() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    // a destination outside the 104x104 build area is never reached
    assert!(!c.tryMove(5, 5, 200, 200, false, 0, 0, 0, 0, 0, 0));
    assert_eq!(c.out.pos, 0);
}
