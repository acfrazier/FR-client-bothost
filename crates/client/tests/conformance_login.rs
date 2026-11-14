//! Java 274 Client.java:7902-7925: LAST_LOGIN_INFO opens the welcome layer.
use client::client::{Client, ClientConfig};
use client::config::IfType;
use client::io::{Packet, ServerProt};

#[test]
fn r274_last_login_info_opens_only_the_java_welcome_layer() {
    for (recovery_days, warning, welcome_code) in [(201, 0, 650), (201, 1, 655), (0, 0, 655)] {
        for (ip, main_modal, has_welcome) in [
            (0x01020304, -1, true),
            (0, -1, true),
            (0x01020304, 7, true),
            (0x01020304, -1, false),
        ] {
            let mut c = Client::new(ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: std::env::temp_dir().display().to_string(),
                members: true,
                lowmem: false,
            });
            if has_welcome {
                for (id, code) in [(2, 650), (3, 655)] {
                    c.set_iface(
                        id,
                        IfType {
                            id: id as i32,
                            layer_id: id as i32 + 40,
                            client_code: code,
                            ..Default::default()
                        },
                    );
                }
            }
            c.main_modal_id = main_modal;
            c.side_modal_id = 8;
            c.chat_modal_id = 9;
            c.report_abuse_input = "stale".into();
            c.report_abuse_mute_option = true;
            c.resumed_pause_button = true;
            c.dialog_input_open = true;
            c.redraw_frame = false;
            c.redraw_side = false;
            c.redraw_chat = false;
            c.redraw_icons = false;
            c.ingame = true;
            c.ptype = ServerProt::LAST_LOGIN_INFO;
            c.psize = 10;
            let mut packet = Packet::new(vec![0; 10]);
            packet.p4(ip);
            packet.p2(9);
            packet.p1(recovery_days);
            packet.p2(10);
            packet.p1(warning);
            packet.pos = 0;
            c.handle_packet(ServerProt::LAST_LOGIN_INFO, &mut packet);
            assert_eq!(packet.pos, 10);
            assert_eq!(c.ptype, -1);
            assert!(c.ingame);
            assert_eq!(c.last_login_ip, ip);
            assert_eq!(c.days_since_login, 9);
            assert_eq!(c.days_since_recovery_change, recovery_days);
            assert_eq!(c.last_login_message_count, 10);
            assert_eq!(c.members_warning, warning);
            assert!(!c.redraw_frame);
            assert!(c.dialog_input_open, "closeModal retains the count dialog");
            if ip != 0 && main_modal == -1 {
                let expected = if !has_welcome {
                    -1
                } else if welcome_code == 650 {
                    42
                } else {
                    43
                };
                assert_eq!(c.main_modal_id, expected);
                assert_eq!(c.welcome_interface_id, expected);
                assert_eq!((c.side_modal_id, c.chat_modal_id), (-1, -1));
                assert!(c.redraw_side && c.redraw_chat && c.redraw_icons);
                assert!(!c.resumed_pause_button);
                assert!(c.report_abuse_input.is_empty());
                assert!(!c.report_abuse_mute_option);
                assert_eq!(&c.out.data()[..c.out.pos], &[51]);
            } else {
                assert_eq!(c.main_modal_id, main_modal);
                assert_eq!((c.side_modal_id, c.chat_modal_id), (8, 9));
                assert!(!c.redraw_side && !c.redraw_chat && !c.redraw_icons);
                assert!(c.resumed_pause_button);
                assert_eq!(c.report_abuse_input, "stale");
                assert!(c.report_abuse_mute_option);
                assert_eq!(c.out.pos, 0);
            }
        }
    }
}
