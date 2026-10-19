//! Primary 289 input telemetry; no server policy or renderer ownership.
use super::client::Client;
use crate::io::ClientProt;

#[derive(Clone, Copy, Default)]
pub(crate) struct Outbound289 {
    camera_delay: u8,
    camera_pending: bool,
    blurred: bool,
    last_click_time: i64,
    pub(crate) cyclelogic4: u16,
    pub(crate) cyclelogic5: u8,
    mouse_x: i32,
    mouse_y: i32,
    duplicates: i32,
}

impl Client {
    pub(crate) fn cold_login_input_289(&mut self) {
        // J:8411-8415,8430. Last coordinate/camera/cycle counters persist.
        self.outbound_289.last_click_time = 0;
        self.outbound_289.duplicates = 0;
        self.outbound_289.blurred = false;
        self.shell.focused = true;
        self.shell.idle_cycles = 0;
        if let Some(shared) = &self.shell.mouse_samples {
            shared.lock().unwrap().samples.clear();
        }
    }

    pub(crate) fn input_packets_289(&mut self) {
        let mut mouse_bytes = [0u8; 244];
        let mut mouse_body_len = 0;
        let mut mouse_sample_count = 0;
        let mut mouse_state = self.outbound_289;
        let mut mouse_packet = false;
        let mouse_samples = self.shell.mouse_samples.clone();
        let mut clear_mouse_samples = false;
        if let Some(shared) = mouse_samples.as_ref() {
            let recorder = shared.lock().unwrap();
            if !self.mouse_tracked {
                clear_mouse_samples = true;
            } else if self.shell.mouse_click_button != 0 || recorder.samples.len() >= 40 {
                mouse_packet = true;
                // J:5829: test the forward body budget before each whole sample,
                // retaining leftovers for the next packet.
                for &(raw_x, raw_y) in &recorder.samples {
                    if mouse_body_len >= 240 {
                        break;
                    }
                    let (x, y, coordinate) = if (raw_x, raw_y) == (-1, -1) {
                        (-1, -1, 524287)
                    } else {
                        let x = raw_x.clamp(0, 764);
                        let y = raw_y.clamp(0, 502);
                        (x, y, y * 765 + x)
                    };
                    if (x, y) == (mouse_state.mouse_x, mouse_state.mouse_y) {
                        mouse_sample_count += 1;
                        mouse_state.duplicates = (mouse_state.duplicates + 1).min(2047);
                        continue;
                    }

                    let dx = x - mouse_state.mouse_x;
                    let dy = y - mouse_state.mouse_y;
                    let encoded_len = if mouse_state.duplicates < 8 {
                        if (-32..=31).contains(&dx) && (-32..=31).contains(&dy) {
                            2
                        } else {
                            3
                        }
                    } else {
                        4
                    };
                    mouse_sample_count += 1;
                    mouse_state.mouse_x = x;
                    mouse_state.mouse_y = y;
                    if encoded_len == 2 {
                        let value = (mouse_state.duplicates << 12) + ((dx + 32) << 6) + dy + 32;
                        let encoded = (value as u16).to_be_bytes();
                        mouse_bytes[mouse_body_len..mouse_body_len + 2].copy_from_slice(&encoded);
                        mouse_body_len += 2;
                    } else if encoded_len == 3 {
                        let encoded =
                            ((mouse_state.duplicates << 19) + coordinate + 8388608).to_be_bytes();
                        mouse_bytes[mouse_body_len..mouse_body_len + 3]
                            .copy_from_slice(&encoded[1..]);
                        mouse_body_len += 3;
                    } else {
                        let encoded = ((mouse_state.duplicates << 19) + coordinate - 1073741824)
                            .to_be_bytes();
                        mouse_bytes[mouse_body_len..mouse_body_len + 4].copy_from_slice(&encoded);
                        mouse_body_len += 4;
                    }
                    mouse_state.duplicates = 0;
                }
            }
        }

        let mouse_opcode = self.client_opcode(ClientProt::EVENT_MOUSE_MOVE);
        let click_packet = self.shell.mouse_click_button != 0;
        let click_time = self.shell.mouse_click_time;
        let click_elapsed =
            ((click_time - self.outbound_289.last_click_time) / 50).min(4095) as i32;
        let click_coordinate =
            self.shell.mouse_click_y.clamp(0, 502) * 765 + self.shell.mouse_click_x.clamp(0, 764);
        let click_button = i32::from(self.shell.mouse_click_button == 2);
        let click_opcode = self.client_opcode(ClientProt::EVENT_MOUSE_CLICK);

        let mut camera_delay = self.outbound_289.camera_delay.saturating_sub(1);
        let mut camera_pending = self.outbound_289.camera_pending;
        if self.shell.key_held[1..=4].contains(&1) {
            camera_pending = true;
        }
        let camera_packet = camera_pending && camera_delay == 0;
        if camera_packet {
            camera_delay = 20;
            camera_pending = false;
        }
        let camera_opcode = self.client_opcode(ClientProt::EVENT_CAMERA_POSITION);
        let camera_pitch = self.orbit_camera_pitch;
        let camera_yaw = self.orbit_camera_yaw;
        let focus_packet = self.outbound_289.blurred == self.shell.focused;
        let focus_state = self.shell.focused;
        let focus_opcode = self.client_opcode(ClientProt::EVENT_APPLET_FOCUS);

        let accepted = self.encode_output(|out| {
            if mouse_packet {
                out.p1_enc(mouse_opcode);
                out.p1(mouse_body_len as i32);
                out.pdata(&mouse_bytes, 0, mouse_body_len);
            }
            if click_packet {
                out.p1_enc(click_opcode);
                out.p4((click_elapsed << 20) + (click_button << 19) + click_coordinate);
            }
            if camera_packet {
                out.p1_enc(camera_opcode);
                out.p2(camera_pitch);
                out.p2(camera_yaw);
            }
            if focus_packet {
                out.p1_enc(focus_opcode);
                out.p1(i32::from(focus_state));
            }
        });
        if accepted {
            if mouse_packet {
                self.outbound_289.mouse_x = mouse_state.mouse_x;
                self.outbound_289.mouse_y = mouse_state.mouse_y;
                self.outbound_289.duplicates = mouse_state.duplicates;
            }
            if let Some(shared) = mouse_samples {
                let mut recorder = shared.lock().unwrap();
                if clear_mouse_samples {
                    recorder.samples.clear();
                } else if mouse_packet {
                    for _ in 0..mouse_sample_count {
                        recorder.samples.pop_front();
                    }
                }
            }
            if click_packet {
                self.outbound_289.last_click_time = click_time;
            }
            self.outbound_289.camera_delay = camera_delay;
            self.outbound_289.camera_pending = camera_pending;
            if focus_packet {
                self.outbound_289.blurred = !focus_state;
            }
        }
    }
}

#[cfg(test)]
mod output_boundary_tests {
    use super::*;
    use crate::client::ClientConfig;

    fn client_with_mouse_samples() -> Client {
        let mut client = Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        });
        client.set_external_reconnect_owner(true);
        client.mouse_tracked = true;
        client.shell.mouse_samples = Some(Default::default());
        let samples = client.shell.mouse_samples.as_ref().unwrap().clone();
        {
            let mut recorder = samples.lock().unwrap();
            recorder.samples.extend([(1, 1), (1, 1), (100, 100)]);
            for _ in 0..8 {
                recorder.samples.push_back((100, 100));
            }
            recorder.samples.push_back((200, 100));
            for _ in 0..28 {
                recorder.samples.push_back((200, 100));
            }
            assert_eq!(recorder.samples.len(), 40);
        }
        client.out.random = Some(crate::io::Isaac::new(&[13, 14, 15, 16]));
        client
    }

    fn fill_output_prefix(client: &mut Client, length: usize) {
        const ZEROES: [u8; 5000] = [0; 5000];
        assert!(client.encode_output(|out| out.pdata(&ZEROES, 0, length)));
    }

    #[test]
    fn mouse_samples_are_reserved_and_consumed_as_complete_records() {
        let mut refused = client_with_mouse_samples();
        let samples = refused.shell.mouse_samples.as_ref().unwrap().clone();
        fill_output_prefix(&mut refused, 4990);
        let before_pos = refused.out.pos;
        let before_bytes = refused.out.data()[..before_pos].to_vec();
        let before_state = refused.outbound_289;
        let mut expected_cipher = refused.out.random.as_ref().unwrap().clone();
        let first_cipher = expected_cipher.next_int();

        refused.input_packets_289();

        assert_eq!(refused.out.pos, before_pos);
        assert_eq!(&refused.out.data()[..before_pos], before_bytes.as_slice());
        assert_eq!(
            refused.out.random.as_ref().unwrap().clone().next_int(),
            first_cipher
        );
        assert_eq!(refused.outbound_289.mouse_x, before_state.mouse_x);
        assert_eq!(refused.outbound_289.mouse_y, before_state.mouse_y);
        assert_eq!(refused.outbound_289.duplicates, before_state.duplicates);
        assert_eq!(samples.lock().unwrap().samples.len(), 40);

        let mut exact = client_with_mouse_samples();
        let samples = exact.shell.mouse_samples.as_ref().unwrap().clone();
        fill_output_prefix(&mut exact, 4989);
        let mut expected_cipher = exact.out.random.as_ref().unwrap().clone();
        let expected_opcode = exact
            .client_opcode(ClientProt::EVENT_MOUSE_MOVE)
            .wrapping_add(expected_cipher.next_int()) as u8;
        let next_cipher = expected_cipher.next_int();

        exact.input_packets_289();

        assert_eq!(exact.out.pos, 5000);
        assert_eq!(exact.out.data()[4989], expected_opcode);
        assert_eq!(exact.out.data()[4990], 9);
        assert_eq!(
            &exact.out.data()[4991..5000],
            &[0x08, 0x61, 0x89, 0x2B, 0x38, 0xC0, 0x41, 0x2B, 0x9C]
        );
        assert_eq!(
            exact.out.random.as_ref().unwrap().clone().next_int(),
            next_cipher,
            "a successful telemetry action encodes its opcode once"
        );
        assert_eq!(exact.outbound_289.mouse_x, 200);
        assert_eq!(exact.outbound_289.mouse_y, 100);
        assert_eq!(exact.outbound_289.duplicates, 28);
        assert!(samples.lock().unwrap().samples.is_empty());
    }

    #[test]
    fn refused_untracked_mouse_clear_preserves_samples() {
        let mut client = client_with_mouse_samples();
        client.mouse_tracked = false;
        client.shell.mouse_click_button = 1;
        client.shell.mouse_click_x = 12;
        client.shell.mouse_click_y = 34;
        let samples = client.shell.mouse_samples.as_ref().unwrap().clone();
        fill_output_prefix(&mut client, 4999);
        let mut expected_cipher = client.out.random.as_ref().unwrap().clone();
        let first_cipher = expected_cipher.next_int();

        client.input_packets_289();

        assert_eq!(client.out.pos, 4999);
        assert_eq!(
            client.out.random.as_ref().unwrap().clone().next_int(),
            first_cipher
        );
        assert_eq!(samples.lock().unwrap().samples.len(), 40);
    }
}
