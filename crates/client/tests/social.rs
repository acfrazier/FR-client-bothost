// Public chat is WordPacked on send and the own echo is sentence-cased
// and filtered (Task 3). The /tmp cache has no packs, so `Client::new`
// falls back to `Cache::default()` and never touches the network (the
// /crc fetch on 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig, ClientPlayer};
use client::io::{ClientProt, Packet, ServerProt};
use client::util::JString;
use client::wordfilter::WordPack;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn to_sentence_case_after_punct() {
    assert_eq!(
        JString::to_sentence_case("hello. world!"),
        "Hello. World!"
    );
}

#[test]
fn message_public_packs_hello() {
    let mut c = client();
    c.ingame = true;
    let mut player = ClientPlayer::at(1, 1);
    player.name = Some("Bob".into());
    c.local_player = Some(player);
    for ch in b"hello" {
        c.shell.apply_key(true, 0, *ch as i32);
    }
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(c.out.data()[0] as i32, ClientProt::MESSAGE_PUBLIC.id & 0xff);
    assert_eq!(c.out.data()[2], 0); // colour
    assert_eq!(c.out.data()[3], 0); // effect
    let packed_len = c.out.data()[1] as usize - 2; // size minus colour+effect
    let mut tail = Packet::new(c.out.data()[4..4 + packed_len].to_vec());
    // WordPack.unpack of a packed "hello" is "Hello " (trailing carry
    // space, oracle 61 bb 40); the echo below is the sentence-cased text.
    assert_eq!(WordPack::unpack(&mut tail, packed_len), "Hello ");
    assert_eq!(c.chat_text[0], "Hello"); // sentence case + filter echo
}

/// A `PLAYER_INFO` frame that sends a `CHAT` mask (0x40) for player index
/// 0: local block op 0, an empty old-vis list, a new-vis entry for index 0
/// with the extended bit, the 2047 sentinel, then the extended section —
/// player 0's CHAT with colourEffect 0x0102, type 0, length and the
/// WordPack payload. `players[2047]` is None, so the local player's
/// extended entry is skipped and consumes no bytes.
fn chat_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0x80, 0x00, 0x00, 0x00, 0x7f, 0xf8, 0x40, 0x01, 0x02, 0x00];
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn player_chat_mask_unpacks_and_adds_chat() {
    let mut c = client();
    c.ingame = true;
    let mut player = ClientPlayer::at(0, 0);
    player.name = Some("Bob".into());
    player.ready = true;
    c.players[0] = Some(player);

    let frame = chat_frame(&[0x61, 0xbb, 0x40]); // WordPack "hello"
    c.psize = frame.len() as i32;
    let mut p = Packet::new(frame);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);

    assert!(c.ingame, "frame consumed exactly; no T2 logout");
    let player = c.players[0].as_ref().unwrap();
    // WordPack.unpack("hello") is "Hello "; WordFilter is identity without
    // wordenc, so the trailing carry space survives.
    assert_eq!(player.chat_message.as_deref(), Some("Hello "));
    assert_eq!(player.chat_colour, 0x01); // colourEffect >> 8
    assert_eq!(player.chat_effect, 0x02); // colourEffect & 0xff
    assert_eq!(player.chat_timer, 150);
    assert_eq!(c.chat_text[0], "Hello ");
    assert_eq!(c.chat_type[0], 2); // type 0 -> add_chat(2, name)
    assert_eq!(c.chat_username[0], "Bob");
}

#[test]
fn player_chat_mask_skips_payload_when_disabled() {
    let mut c = client();
    c.ingame = true;
    let mut player = ClientPlayer::at(0, 0);
    player.name = Some("Bob".into());
    player.ready = true;
    c.players[0] = Some(player);
    c.chat_disabled = 1;

    let frame = chat_frame(&[0x61, 0xbb, 0x40]);
    c.psize = frame.len() as i32;
    let mut p = Packet::new(frame);
    c.handle_packet(ServerProt::PLAYER_INFO, &mut p);

    assert!(c.ingame, "payload still skipped; frame stays aligned");
    assert_eq!(c.players[0].as_ref().unwrap().chat_message, None);
    assert!(c.chat_text[0].is_empty(), "disabled chat must not add a line");
}

// ---- Task 4: friends/ignore list state, packets and list ops ----

#[test]
fn update_friendlist_adds_and_login_chat() {
    let mut c = client();
    let hash = JString::to_userhash("bob") as i64;
    let mut p = Packet::new(vec![0; 32]);
    p.p8(hash);
    p.p1(10); // world == node_id default 10
    p.pos = 0;
    c.apply_update_friendlist(&mut p);
    assert_eq!(c.friend_count, 1);
    assert_eq!(c.friend_username[0], "Bob");
    assert_eq!(c.friend_node_id[0], 10);
}

#[test]
fn add_friend_encodes_p8() {
    let mut c = client();
    c.local_player = Some(ClientPlayer::at(5, 5));
    if let Some(p) = c.local_player.as_mut() {
        p.name = Some("Test".into());
    }
    let hash = JString::to_userhash("bob") as i64;
    c.add_friend(hash);
    assert_eq!(c.out.data()[0], ClientProt::FRIENDLIST_ADD.id as u8);
    assert_eq!(c.friend_count, 1);
}

#[test]
fn message_game_trade_skips_ignored() {
    let mut c = client();
    let hash = JString::to_userhash("eve") as i64;
    c.ignore_userhash[0] = hash;
    c.ignore_count = 1;
    let mut p = Packet::new(vec![0; 64]);
    p.pjstr("eve:tradereq:");
    p.pos = 0;
    c.apply_message_game(&mut p);
    assert!(c.chat_text[0].is_empty() || !c.chat_text[0].contains("wishes to trade"));
}

#[test]
fn update_ignorelist_fills_userhashes() {
    let mut c = client();
    let eve = JString::to_userhash("eve") as i64;
    let bob = JString::to_userhash("bob") as i64;
    let mut p = Packet::new(vec![0; 32]);
    p.p8(eve);
    p.p8(bob);
    let psize = p.pos as i32;
    p.pos = 0;
    c.apply_update_ignorelist(&mut p, psize);
    assert_eq!(c.ignore_count, 2);
    assert_eq!(c.ignore_userhash[0], eve);
    assert_eq!(c.ignore_userhash[1], bob);
}

#[test]
fn message_private_unpacks_and_adds_chat() {
    let mut c = client();
    let from = JString::to_userhash("bob") as i64;
    let mut p = Packet::new(vec![0; 64]);
    p.p8(from);
    p.p4(1);
    p.p1(0); // staff level 0
    WordPack::pack(&mut p, "hi");
    let psize = p.pos as i32;
    p.pos = 0;
    c.apply_message_private(&mut p, psize);
    assert_eq!(c.private_message_ids[0], 1);
    assert_eq!(c.private_message_count, 1);
    assert_eq!(c.chat_text[0], "Hi"); // WordPack unpack + identity filter
    assert_eq!(c.chat_username[0], "Bob"); // toScreenName(toRawUsername)
    assert_eq!(c.chat_type[0], 3);
}

#[test]
fn del_friend_removes_and_encodes() {
    let mut c = client();
    let bob = JString::to_userhash("bob") as i64;
    let eve = JString::to_userhash("eve") as i64;
    c.friend_userhash[0] = bob;
    c.friend_username[0] = "Bob".into();
    c.friend_userhash[1] = eve;
    c.friend_username[1] = "Eve".into();
    c.friend_count = 2;
    c.del_friend(bob);
    assert_eq!(c.friend_count, 1);
    assert_eq!(c.out.data()[0], ClientProt::FRIENDLIST_DEL.id as u8);
    assert_eq!(c.friend_userhash[0], eve);
    assert_eq!(c.friend_username[0], "Eve");
}
