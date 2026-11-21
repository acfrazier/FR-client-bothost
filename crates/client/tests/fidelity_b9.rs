#[path = "common/isolation.rs"]
mod isolation;

use client::client::{ClientConfig, ClientRevision};

#[test]
fn fidelity_b9_private_message_accepts_zero_userhash() {
    for (revision, opcode) in [
        (ClientRevision::R274, 139),
        (ClientRevision::R289, 107),
        (ClientRevision::R377, 227),
    ] {
        let mut c = isolation::IsolatedClient::with_revision(
            ClientConfig {
                host: String::new(),
                port: 0,
                cache_dir: String::new(),
                members: true,
                lowmem: true,
            },
            revision,
        );
        c.social_input_open = true;
        c.social_input_type = 3;
        c.social_input = "Hi".into();
        c.social_userhash = 0;
        c.consume_chat_key(13);
        assert_eq!(
            &c.out.data()[..c.out.pos],
            &[opcode, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0x65],
            "{revision:?}"
        );
        assert_eq!(c.chat_text[0], "Hi");
        assert_eq!(c.chat_type[0], 6);
        assert!(!c.social_input_open);
    }
}
