#[path = "common/isolation.rs"]
mod isolation;

use client::client::{ClientConfig, ClientRevision, MiniMenuAction};

#[test]
fn fidelity_b8_close_only_rearms_continue_for_side_or_chat() {
    for (revision, close, resume) in [
        (ClientRevision::R274, 51, 72),
        (ClientRevision::R289, 93, 166),
        (ClientRevision::R377, 110, 226),
    ] {
        for (side, chat, main) in [
            (-1, -1, -1),
            (-1, -1, 7),
            (8, -1, -1),
            (-1, 9, -1),
            (8, 9, 7),
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
            c.side_modal_id = side;
            c.chat_modal_id = chat;
            c.main_modal_id = main;
            c.resumed_pause_button = true;
            c.menu_action[0] = MiniMenuAction::CLOSE_BUTTON;
            c.doAction(0);
            assert_eq!(&c.out.data()[..c.out.pos], &[close]);
            c.out.pos = 0;
            c.menu_action[0] = MiniMenuAction::PAUSE_BUTTON;
            c.menu_param_c[0] = 5;
            c.doAction(0);
            let expected: &[u8] = if side != -1 || chat != -1 {
                &[resume, 0, 5]
            } else {
                &[]
            };
            assert_eq!(
                &c.out.data()[..c.out.pos],
                expected,
                "{revision:?}: side={side}, chat={chat}, main={main}"
            );
            assert_eq!(
                (c.side_modal_id, c.chat_modal_id, c.main_modal_id),
                (-1, -1, -1)
            );
        }
    }
}
