use client::io::{ClientProt, ServerProt, SERVER_PROT_SIZES};

#[test]
fn client_opcodes_and_lengths() {
    assert_eq!(ClientProt::NO_TIMEOUT.id, 120);
    assert_eq!(ClientProt::NO_TIMEOUT.length, 0);
    assert_eq!(ClientProt::MOVE_GAMECLICK.id, 207);
    assert_eq!(ClientProt::MOVE_GAMECLICK.length, -1);
    assert_eq!(ClientProt::OPNPC2.id, 233);
    assert_eq!(ClientProt::OPNPC2.length, 2);
    assert_eq!(ClientProt::IF_BUTTON.id, 9);
    assert_eq!(ClientProt::IF_BUTTON.length, 2);
}

#[test]
fn server_opcodes_and_size_table() {
    assert_eq!(ServerProt::REBUILD_NORMAL, 231);
    assert_eq!(SERVER_PROT_SIZES[231], 4);
    assert_eq!(ServerProt::PLAYER_INFO, 167);
    assert_eq!(SERVER_PROT_SIZES[167], -2);
    assert_eq!(ServerProt::MIDI_SONG, 23);
    assert_eq!(SERVER_PROT_SIZES[23], 2);
    assert_eq!(ServerProt::MIDI_JINGLE, 15);
    assert_eq!(SERVER_PROT_SIZES[15], 4);
    assert_eq!(ServerProt::LOGOUT, 88);
    assert_eq!(SERVER_PROT_SIZES[88], 0);
    assert_eq!(SERVER_PROT_SIZES.len(), 256);
}
