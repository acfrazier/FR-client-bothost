//! Revision-377 outbound packet writers and session input telemetry.
//!
//! Packet ids and field layouts are taken from Client-Java 377. Keep these
//! helpers independent of the 274/289 `map_client_prot` translation: each
//! in-game emitter uses the generated revision-377 packet descriptor, while
//! the login wrapper is a raw, unencrypted handshake frame.

use super::client::Client;
use crate::io::{ClientProt377, Packet};
use std::cell::Cell;
use std::collections::VecDeque;

/// Session input state copied from the Java 377 client's persistent fields.
#[derive(Default)]
pub(crate) struct Outbound377 {
    camera_delay: u8,
    camera_pending: bool,
    focus: bool,
    last_click_time: i64,
    mouse_x: i32,
    mouse_y: i32,
    duplicates: i32,
    cyclelogic1: i32,
    cyclelogic2: i32,
    cyclelogic3: i32,
    cyclelogic4: i32,
    cyclelogic5: i32,
    cyclelogic6: i32,
    /// Set by revision-gated shared emitters before the session flush gate.
    pub(crate) unmapped: Cell<Option<i32>>,
}

impl Client {
    /// Java `getTopLevel`: count only while the camera pitch is below 310.
    ///
    /// Java: `Client.java:6227-6247`.
    pub(crate) fn top_level_packet_377(&mut self, camera_pitch: i32) {
        if camera_pitch >= 310 {
            return;
        }
        self.outbound.r377_mut().cyclelogic1 += 1;
        if self.outbound.r377_mut().cyclelogic1 > 1457 {
            self.outbound.r377_mut().cyclelogic1 = 0;
            let random_word = (super::client_build::random_float() * 65536.0) as u16;
            let random_byte1 = (super::client_build::random_float() * 256.0) as u8;
            let include_optional_word = (super::client_build::random_float() * 2.0) as i32 == 0;
            let random_byte2 = (super::client_build::random_float() * 256.0) as u8;
            anticheat_cyclelogic1_377(
                &mut self.out,
                random_word,
                random_byte1,
                include_optional_word,
                random_byte2,
            );
        }
    }

    /// Java `pushProjectiles` increments on each call and emits every 52 calls.
    ///
    /// Java: `Client.java:6137-6171`.
    pub(crate) fn projectiles_packet_377(&mut self) {
        self.outbound.r377_mut().cyclelogic5 += 1;
        if self.outbound.r377_mut().cyclelogic5 > 51 {
            self.outbound.r377_mut().cyclelogic5 = 0;
            anticheat_cyclelogic5_377(&mut self.out);
        }
    }

    /// Java `draw3DEntityElements` counts only while the reboot timer is active.
    ///
    /// Java: `Client.java:6683-6698`.
    pub(crate) fn system_update_packet_377(&mut self) {
        if self.reboot_timer == 0 {
            return;
        }
        self.outbound.r377_mut().cyclelogic3 += 1;
        if self.outbound.r377_mut().cyclelogic3 > 112 {
            self.outbound.r377_mut().cyclelogic3 = 0;
            anticheat_cyclelogic3_377(&mut self.out);
        }
    }

    /// Java `handleChatModeInput` returns unless the current click is left.
    ///
    /// Java: `Client.java:4512-4563`.
    pub(crate) fn chat_mode_packet_377(&mut self) {
        if self.shell.mouse_click_button != 1 {
            return;
        }
        self.outbound.r377_mut().cyclelogic6 += 1;
        if self.outbound.r377_mut().cyclelogic6 > 161 {
            self.outbound.r377_mut().cyclelogic6 = 0;
            anticheat_cyclelogic6_377(&mut self.out);
        }
    }

    /// Java `mainredraw` increments this counter only inside `redrawFrame`.
    ///
    /// Java: `Client.java:5720-5747`.
    pub(crate) fn redraw_frame_packet_377(&mut self) {
        self.outbound.r377_mut().cyclelogic2 += 1;
        if self.outbound.r377_mut().cyclelogic2 > 85 {
            self.outbound.r377_mut().cyclelogic2 = 0;
            anticheat_cyclelogic2_377(&mut self.out);
        }
    }

    /// Java `createMinimap` increments after its minimap location drawing pass.
    ///
    /// Java: `Client.java:3708-3750`.
    pub(crate) fn minimap_buffer_packet_377(&mut self) {
        self.outbound.r377_mut().cyclelogic4 += 1;
        if self.outbound.r377_mut().cyclelogic4 > 177 {
            self.outbound.r377_mut().cyclelogic4 = 0;
            anticheat_cyclelogic4_377(&mut self.out);
        }
    }
}

/// Emit Java `ANTICHEAT_CYCLELOGIC2` from the full-frame redraw counter.
///
/// Java: `Client.java:5742-5747`.
fn anticheat_cyclelogic2_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC2.id);
}

/// Emit Java `ANTICHEAT_CYCLELOGIC4` from the completed minimap build.
///
/// Java: `Client.java:3744-3749`.
fn anticheat_cyclelogic4_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC4.id);
    packet.p3(2657152);
}

/// Emit Java's bodyless `ANTICHEAT_CYCLELOGIC5` after projectile processing.
///
/// Java: `Client.java:6166-6171`.
fn anticheat_cyclelogic5_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC5.id);
}

impl Client {
    /// Reset the Java input fields initialized after login acceptance.
    ///
    /// Java: `Client.java:2742-2746,2761-2762`.
    pub(crate) fn cold_login_input_377(&mut self) {
        self.outbound.r377_mut().last_click_time = 0;
        self.outbound.r377_mut().duplicates = 0;
        self.outbound.r377_mut().focus = true;
        self.shell.focused = true;
        self.shell.idle_cycles = 0;
        if let Some(shared) = &self.shell.mouse_samples {
            shared.lock().unwrap().samples.clear();
        }
    }

    fn mouse_packet_377(&mut self) {
        let tracked = self.mouse_tracked;
        let click_button = self.shell.mouse_click_button;
        if let Some(shared) = &self.shell.mouse_samples {
            let mut recorder = shared.lock().unwrap();
            mouse_move_377(
                &mut self.out,
                self.outbound.r377_mut(),
                tracked,
                click_button,
                Some(&mut recorder.samples),
            );
        } else {
            mouse_move_377(
                &mut self.out,
                self.outbound.r377_mut(),
                tracked,
                click_button,
                None,
            );
        }
    }

    /// Emit the ordinary Java input packets for one in-game session pass.
    ///
    /// Java: `Client.java:3033-3159`.
    pub(crate) fn input_packets_377(&mut self) {
        self.mouse_packet_377();
        if self.shell.mouse_click_button != 0 {
            mouse_click_377(
                &mut self.out,
                self.outbound.r377_mut(),
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                self.shell.mouse_click_time,
            );
        }

        self.outbound.r377_mut().camera_delay =
            self.outbound.r377_mut().camera_delay.saturating_sub(1);
        if self.shell.key_held[1..=4].contains(&1) {
            self.outbound.r377_mut().camera_pending = true;
        }
        if self.outbound.r377_mut().camera_pending && self.outbound.r377_mut().camera_delay == 0 {
            self.outbound.r377_mut().camera_delay = 20;
            self.outbound.r377_mut().camera_pending = false;
            camera_position_377(
                &mut self.out,
                self.orbit_camera_pitch,
                self.orbit_camera_yaw,
            );
        }

        if self.outbound.r377_mut().focus != self.shell.focused {
            self.outbound.r377_mut().focus = self.shell.focused;
            focus_377(&mut self.out, self.shell.focused);
        }
    }
}

/// Emit one variable-length Java movement packet body.
///
/// `kind` is Java `tryMove`'s type: 0 game click, 1 minimap click, 2 op click.
/// The caller supplies at most 24 `(dx, dz)` waypoint deltas in wire order.
/// For kind 1, the caller appends Java's 14-byte minimap suffix after this
/// body. The length byte includes that suffix.
///
/// Java: `Client.java:7182-7206`.
pub(crate) fn movement_377(
    packet: &mut Packet,
    kind: i32,
    absolute_x: i32,
    absolute_z: i32,
    run: bool,
    deltas: impl ExactSizeIterator<Item = (i32, i32)>,
) -> bool {
    let (opcode, suffix_length) = match kind {
        0 => (ClientProt377::MOVE_GAMECLICK, 0),
        1 => (ClientProt377::MOVE_MINIMAPCLICK, 14),
        2 => (ClientProt377::MOVE_OPCLICK, 0),
        _ => return false,
    };
    packet.p1_enc(opcode.id);
    packet.p1((deltas.len() * 2 + 5 + suffix_length) as i32);
    p2_alt3(packet, absolute_x);
    packet.p1(i32::from(run));
    p2_alt3(packet, absolute_z);
    for (dx, dz) in deltas {
        packet.p1(dx);
        p1_alt3(packet, dz);
    }
    true
}

/// Java Client.java:10183-10189: x is big-endian with a biased low byte,
/// followed by little-endian z and location ID.
pub(crate) fn oploc1_377(packet: &mut Packet, x: i32, z: i32, location: i32) {
    packet.p1_enc(ClientProt377::OPLOC1.id);
    packet.p1(x >> 8);
    packet.p1(x.wrapping_add(128));
    p2_alt1(packet, z);
    p2_alt1(packet, location);
}
/// Emit Java `OPLOCU`: selected item on a location.
///
/// Java: `Client.java:9770-9779`.
pub(crate) fn oplocu_377(
    packet: &mut Packet,
    location: i32,
    selected_interface: i32,
    item_id: i32,
    z: i32,
    selected_slot: i32,
    x: i32,
) {
    packet.p1_enc(ClientProt377::OPLOCU.id);
    p2_alt1(packet, location);
    p2_alt1(packet, selected_interface);
    p2_alt1(packet, item_id);
    p2_alt1(packet, z);
    packet.p2(selected_slot);
    p2_alt3(packet, x);
}

/// Emit Java `OPHELDU`: selected item on another inventory item.
///
/// Java: `Client.java:9922-9940`.
pub(crate) fn opheldu_377(
    packet: &mut Packet,
    target_item_id: i32,
    selected_slot: i32,
    item_id: i32,
    selected_interface: i32,
    target_slot: i32,
    target_interface: i32,
) {
    packet.p1_enc(ClientProt377::OPHELDU.id);
    packet.p2(target_item_id);
    p2_alt1(packet, selected_slot);
    p2_alt1(packet, item_id);
    p2_alt3(packet, selected_interface);
    p2_alt2(packet, target_slot);
    p2_alt2(packet, target_interface);
}


/// Emit Java `OPNPC1`: transformed NPC index.
///
/// Java: `Client.java:10456-10459`.
pub(crate) fn opnpc1_377(packet: &mut Packet, index: i32) {
    packet.p1_enc(ClientProt377::OPNPC1.id);
    p2_alt1(packet, index);
}

/// Emit Java `RESUME_PAUSEBUTTON`: plain component id.
///
/// Java: `Client.java:10252-10256`.
pub(crate) fn resume_pausebutton_377(packet: &mut Packet, component: i32) {
    packet.p1_enc(ClientProt377::RESUME_PAUSEBUTTON.id);
    packet.p2(component);
}

/// Emit Java `IDK_SAVEDESIGN` with the already selected wire gender.
///
/// Java: `Client.java:12030-12040`.
pub(crate) fn idk_savedesign_377(
    packet: &mut Packet,
    wire_gender: i32,
    parts: &[i32; 7],
    colours: &[i32; 5],
) {
    packet.p1_enc(ClientProt377::IDK_SAVEDESIGN.id);
    packet.p1(wire_gender);
    for &part in parts {
        packet.p1(part);
    }
    for &colour in colours {
        packet.p1(colour);
    }
}

/// Emit Java's bodyless `IDLE_TIMER` packet.
///
/// Java: `Client.java:3338-3345`.
pub(crate) fn idle_timer_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::IDLE_TIMER.id);
}

/// Emit the Java `ANTICHEAT_CYCLELOGIC1` random payload.
///
/// Random values are supplied in Java draw order. `include_optional_word`
/// represents the `(int) (Math.random() * 2.0D) == 0` branch.
///
/// Java: `Client.java:6227-6247`.
pub(crate) fn anticheat_cyclelogic1_377(
    packet: &mut Packet,
    random_word: u16,
    random_byte1: u8,
    include_optional_word: bool,
    random_byte2: u8,
) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC1.id);
    packet.p1(0);
    let start = packet.pos;
    packet.p1(219);
    packet.p2(37745);
    packet.p1(61);
    packet.p2(43756);
    packet.p2(i32::from(random_word));
    packet.p1(i32::from(random_byte1));
    packet.p2(51171);
    if include_optional_word {
        packet.p2(15808);
    }
    packet.p1(97);
    packet.p1(i32::from(random_byte2));
    packet.psize1((packet.pos - start) as i32);
}

/// Emit Java `ANTICHEAT_CYCLELOGIC3` with its four zero bytes.
///
/// Java: `Client.java:6692-6698`.
pub(crate) fn anticheat_cyclelogic3_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC3.id);
    packet.p4(0);
}

/// Emit Java `ANTICHEAT_CYCLELOGIC6` from chat-mode input.
///
/// This is not the unrelated arrival-clear callback used by the 274/289
/// renderer path. Java emits `38304` from `handleChatModeInput`.
///
/// Java: `Client.java:4557-4563`.
pub(crate) fn anticheat_cyclelogic6_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_CYCLELOGIC6.id);
    packet.p2(38304);
}

fn mouse_move_377(
    packet: &mut Packet,
    state: &mut Outbound377,
    tracked: bool,
    click_button: i32,
    mut samples: Option<&mut VecDeque<(i32, i32)>>,
) {
    if !tracked {
        if let Some(samples) = samples {
            samples.clear();
        }
        return;
    }
    let sample_count = samples.as_ref().map_or(0, |samples| samples.len());
    if click_button == 0 && sample_count < 40 {
        return;
    }

    packet.p1_enc(ClientProt377::EVENT_MOUSE_MOVE.id);
    packet.p1(0);
    let start = packet.pos;
    if let Some(samples) = samples.as_mut() {
        while let Some((raw_x, raw_y)) = samples.pop_front() {
            let (x, y, coordinate) = if (raw_x, raw_y) == (-1, -1) {
                (-1, -1, 524287)
            } else {
                let x = raw_x.clamp(0, 764);
                let y = raw_y.clamp(0, 502);
                (x, y, y * 765 + x)
            };
            if (x, y) == (state.mouse_x, state.mouse_y) {
                state.duplicates = (state.duplicates + 1).min(2047);
                continue;
            }
            let dx = x - state.mouse_x;
            let dy = y - state.mouse_y;
            state.mouse_x = x;
            state.mouse_y = y;
            if state.duplicates < 8 && (-32..=31).contains(&dx) && (-32..=31).contains(&dy) {
                packet.p2((state.duplicates << 12) + ((dx + 32) << 6) + dy + 32);
            } else if state.duplicates < 8 {
                packet.p3((state.duplicates << 19) + 8388608 + coordinate);
            } else {
                packet.p4((state.duplicates << 19) - 1073741824 + coordinate);
            }
            state.duplicates = 0;
        }
    }
    packet.psize1((packet.pos - start) as i32);
}

fn mouse_click_377(
    packet: &mut Packet,
    state: &mut Outbound377,
    button: i32,
    x: i32,
    y: i32,
    click_time: i64,
) {
    let elapsed = ((click_time - state.last_click_time) / 50).min(4095) as i32;
    state.last_click_time = click_time;
    let x = x.clamp(0, 764);
    let y = y.clamp(0, 502);
    let coordinate = y * 765 + x;
    let button = i32::from(button == 2);
    packet.p1_enc(ClientProt377::EVENT_MOUSE_CLICK.id);
    packet.p4(elapsed
        .wrapping_shl(20)
        .wrapping_add(button.wrapping_shl(19))
        .wrapping_add(coordinate));
}

fn camera_position_377(packet: &mut Packet, pitch: i32, yaw: i32) {
    packet.p1_enc(ClientProt377::EVENT_CAMERA_POSITION.id);
    p2_alt1(packet, pitch);
    p2_alt1(packet, yaw);
}

fn focus_377(packet: &mut Packet, focused: bool) {
    packet.p1_enc(ClientProt377::EVENT_APPLET_FOCUS.id);
    packet.p1(i32::from(focused));
}

fn p1_alt3(packet: &mut Packet, value: i32) {
    packet.p1(128i32.wrapping_sub(value));
}

fn p2_alt1(packet: &mut Packet, value: i32) {
    packet.p1(value);
    packet.p1(value >> 8);
}

fn p2_alt2(packet: &mut Packet, value: i32) {
    packet.p1(value >> 8);
    packet.p1(value.wrapping_add(128));
}

fn p2_alt3(packet: &mut Packet, value: i32) {
    packet.p1(value.wrapping_add(128));
    packet.p1(value >> 8);
}

/// Build the RSA plaintext login block with the caller's revision-377 UID.
///
/// The block layout is the same as the existing `Client::write_login_block`;
/// the explicit UID lets the caller supply the Java `signlink.uid` value.
/// Apply `Packet::rsaenc` after writing this block.
///
/// Java: `Client.java:2701-2710`.
pub(crate) fn write_login_block(
    block: &mut Packet,
    seed: [i32; 4],
    uid: i32,
    username: &str,
    password: &str,
) {
    block.pos = 0;
    block.p1(10);
    for seed_word in seed {
        block.p4(seed_word);
    }
    block.p4(uid);
    block.pjstr(username);
    block.pjstr(password);
}

/// Build the 377 login wrapper around the already RSA-encoded login block.
///
/// `rsa_block` is the source of the appended ciphertext and must have had
/// `rsaenc` applied already. The wrapper packet is an ordinary, unobserved
/// `Packet`; it is not an ISAAC-framed game packet.
///
/// Java: `Client.java:2711-2724`.
pub(crate) fn write_login_wrapper(
    wrapper: &mut Packet,
    reconnect: bool,
    low_mem: bool,
    checksums: &[i32; 9],
    rsa_block: &Packet,
) {
    wrapper.pos = 0;
    wrapper.p1(if reconnect { 18 } else { 16 });
    wrapper.p1((rsa_block.pos + 40) as i32);
    wrapper.p1(255);
    wrapper.p2(377);
    wrapper.p1(i32::from(low_mem));
    for &checksum in checksums {
        wrapper.p4(checksum);
    }
    wrapper.pdata(rsa_block.data(), 0, rsa_block.pos);
}

/// Emit the revision-377 `NO_TIMEOUT` packet (opcode 40, empty body).
/// Java: `Client.java:3407-3410`; during `buildScene`, `3527-3547`,
/// `3569-3570`, and `3623-3631`.
pub(crate) fn no_timeout(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::NO_TIMEOUT.id);
}

/// Emit `MAP_BUILD_COMPLETE` after a successful scene build (opcode 6).
/// Java: `Client.java:3496-3501`.
pub(crate) fn map_build_complete(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::MAP_BUILD_COMPLETE.id);
}

/// Emit the `IF_BUTTON` frame for the selected interface component.
/// The caller must preserve Java's `clientButton` gate: this is used for the
/// logout button only when the client-code handler arms `logoutTimer` and
/// allows the click to be sent.
///
/// Java emits the interface click at `Client.java:9890-9893`.
pub(crate) fn if_button(packet: &mut Packet, component_id: i32) {
    packet.p1_enc(ClientProt377::IF_BUTTON.id);
    packet.p2(component_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_block_matches_java_377_literal() {
        // J:2701-2710 writes opcode 10, four seed words, signlink.uid,
        // username and password as newline-terminated strings.
        let mut block = Packet::alloc(0);
        write_login_block(
            &mut block,
            [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334],
            0x4142_4344,
            "u",
            "p",
        );

        assert_eq!(
            &block.data()[..block.pos],
            &[
                0x0a, 0x01, 0x02, 0x03, 0x04, 0x11, 0x12, 0x13, 0x14, 0x21, 0x22, 0x23, 0x24, 0x31,
                0x32, 0x33, 0x34, 0x41, 0x42, 0x43, 0x44, b'u', 0x0a, b'p', 0x0a,
            ]
        );
    }

    #[test]
    fn login_wrapper_matches_java_377_literal() {
        // J:2711-2724: 16, length=(RSA block+40), 255, revision 377,
        // low-memory flag, nine big-endian JAG CRCs, then the RSA bytes.
        let mut rsa_block = Packet::alloc(0);
        rsa_block.p1(0xaa);
        rsa_block.p1(0xbb);
        let checksums = [
            0x0102_0304,
            0x1112_1314,
            0x2122_2324,
            0x3132_3334,
            0x4142_4344,
            0x5152_5354,
            0x6162_6364,
            0x7172_7374,
            0x8182_8384u32 as i32,
        ];
        let mut wrapper = Packet::alloc(1);

        write_login_wrapper(&mut wrapper, false, true, &checksums, &rsa_block);

        assert_eq!(
            &wrapper.data()[..wrapper.pos],
            &[
                0x10, 0x2a, 0xff, 0x01, 0x79, 0x01, 0x01, 0x02, 0x03, 0x04, 0x11, 0x12, 0x13, 0x14,
                0x21, 0x22, 0x23, 0x24, 0x31, 0x32, 0x33, 0x34, 0x41, 0x42, 0x43, 0x44, 0x51, 0x52,
                0x53, 0x54, 0x61, 0x62, 0x63, 0x64, 0x71, 0x72, 0x73, 0x74, 0x81, 0x82, 0x83, 0x84,
                0xaa, 0xbb,
            ]
        );
    }

    #[test]
    fn reconnect_wrapper_uses_java_opcode_18() {
        // J:2712-2716 selects 18 for reconnect, with the same 377 wrapper.
        let mut rsa_block = Packet::alloc(0);
        rsa_block.p1(0xaa);
        let mut wrapper = Packet::alloc(1);

        write_login_wrapper(&mut wrapper, true, false, &[0; 9], &rsa_block);

        assert_eq!(&wrapper.data()[..2], &[0x12, 0x29]);
        assert_eq!(&wrapper.data()[2..6], &[0xff, 0x01, 0x79, 0x00]);
        assert_eq!(wrapper.data()[wrapper.pos - 1], 0xaa);
    }

    #[test]
    fn minimum_377_game_packets_match_java_literals() {
        // J:3407-3410 (keepalive), 3496-3501 (scene completion), and
        // 9890-9893 (IF_BUTTON; the logout clientButton path allows it).
        let mut packet = Packet::alloc_outbound(1);
        no_timeout(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[40]);

        packet.pos = 0;
        map_build_complete(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[6]);

        packet.pos = 0;
        if_button(&mut packet, 2458);
        assert_eq!(&packet.data()[..packet.pos], &[79, 0x09, 0x9a]);

        assert_eq!(ClientProt377::NO_TIMEOUT.length, 0);
        assert_eq!(ClientProt377::MAP_BUILD_COMPLETE.length, 0);
        assert_eq!(ClientProt377::IF_BUTTON.length, 2);
    }

    #[test]
    fn first_location_action_matches_java_literal() {
        let mut packet = Packet::alloc_outbound(1);
        oploc1_377(&mut packet, 3098, 3107, 3014);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[181, 0x0c, 0x9a, 0x23, 0x0c, 0xc6, 0x0b]
        );
    }
    #[test]
    fn use_held_on_held_matches_java_377_literal() {
        let mut packet = Packet::alloc_outbound(1);
        opheldu_377(&mut packet, 0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789);

        assert_eq!(
            &packet.data()[..packet.pos],
            &[
                1, 0x12, 0x34, 0x45, 0x23, 0x56, 0x34, 0xe7, 0x45, 0xf8, 0x56, 0x89, 0x67,
            ]
        );
        assert_eq!(ClientProt377::OPHELDU.length, 12);
    }

    #[test]
    fn use_held_on_location_matches_java_377_literal() {
        let mut packet = Packet::alloc_outbound(1);
        oplocu_377(&mut packet, 0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789);

        assert_eq!(
            &packet.data()[..packet.pos],
            &[
                152, 0x34, 0x12, 0x45, 0x23, 0x56, 0x34, 0x67, 0x45, 0x78, 0x56, 0x09, 0x67,
            ]
        );
        assert_eq!(ClientProt377::OPLOCU.length, 12);
    }


    #[test]
    fn movement_writers_match_377_literal_bytes_and_lengths() {
        let mut packet = Packet::alloc_outbound(1);
        assert!(movement_377(
            &mut packet,
            0,
            0x1234,
            0x5678,
            true,
            [(5, -3), (-7, 4)].into_iter(),
        ));
        assert_eq!(
            &packet.data()[..packet.pos],
            &[28, 9, 0xb4, 0x12, 1, 0xf8, 0x56, 5, 0x83, 0xf9, 0x7c]
        );

        packet.pos = 0;
        assert!(movement_377(
            &mut packet,
            1,
            0x1234,
            0x5678,
            false,
            std::iter::empty(),
        ));
        assert_eq!(
            &packet.data()[..packet.pos],
            &[213, 19, 0xb4, 0x12, 0, 0xf8, 0x56]
        );

        packet.pos = 0;
        assert!(movement_377(
            &mut packet,
            2,
            0x1234,
            0x5678,
            true,
            [(5, -3), (-7, 4)].into_iter(),
        ));
        assert_eq!(
            &packet.data()[..packet.pos],
            &[247, 9, 0xb4, 0x12, 1, 0xf8, 0x56, 5, 0x83, 0xf9, 0x7c]
        );

        packet.pos = 0;
        assert!(!movement_377(
            &mut packet,
            3,
            0,
            0,
            false,
            std::iter::empty(),
        ));
        assert_eq!(packet.pos, 0);
        assert_eq!(ClientProt377::MOVE_GAMECLICK.length, -1);
        assert_eq!(ClientProt377::MOVE_MINIMAPCLICK.length, -1);
        assert_eq!(ClientProt377::MOVE_OPCLICK.length, -1);
    }

    #[test]
    fn interaction_writers_match_377_literal_bytes_and_lengths() {
        let mut packet = Packet::alloc_outbound(1);
        opnpc1_377(&mut packet, 0x1234);
        assert_eq!(&packet.data()[..packet.pos], &[112, 0x34, 0x12]);
        assert_eq!(ClientProt377::OPNPC1.length, 2);

        packet.pos = 0;
        resume_pausebutton_377(&mut packet, 0x1234);
        assert_eq!(&packet.data()[..packet.pos], &[226, 0x12, 0x34]);
        assert_eq!(ClientProt377::RESUME_PAUSEBUTTON.length, 2);

        packet.pos = 0;
        idk_savedesign_377(&mut packet, 1, &[1, 2, 3, 4, 5, 6, 7], &[8, 9, 10, 11, 12]);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[163, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
        assert_eq!(ClientProt377::IDK_SAVEDESIGN.length, 13);
    }

    #[test]
    fn session_input_writers_match_377_literal_bytes_and_lengths() {
        let mut packet = Packet::alloc_outbound(1);
        let mut state = Outbound377::default();
        let mut samples = VecDeque::new();
        samples.extend([(5, 7), (5, 7), (40, 60)]);
        mouse_move_377(&mut packet, &mut state, true, 1, Some(&mut samples));
        assert_eq!(
            &packet.data()[..packet.pos],
            &[171, 5, 0x09, 0x67, 0x88, 0xb3, 0x74]
        );
        assert!(samples.is_empty());
        assert_eq!(ClientProt377::EVENT_MOUSE_MOVE.length, -1);

        packet.pos = 0;
        mouse_click_377(&mut packet, &mut state, 2, 1, 2, 50);
        assert_eq!(&packet.data()[..packet.pos], &[19, 0, 0x18, 0x05, 0xfb]);
        assert_eq!(ClientProt377::EVENT_MOUSE_CLICK.length, 4);

        packet.pos = 0;
        camera_position_377(&mut packet, 0x1234, 0x5678);
        assert_eq!(&packet.data()[..packet.pos], &[140, 0x34, 0x12, 0x78, 0x56]);
        assert_eq!(ClientProt377::EVENT_CAMERA_POSITION.length, 4);

        packet.pos = 0;
        focus_377(&mut packet, true);
        assert_eq!(&packet.data()[..packet.pos], &[187, 1]);
        assert_eq!(ClientProt377::EVENT_APPLET_FOCUS.length, 1);

        packet.pos = 0;
        idle_timer_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[202]);
        assert_eq!(ClientProt377::IDLE_TIMER.length, 0);
    }

    #[test]
    fn renderer_anticheat_writers_match_377_java_payloads() {
        let mut packet = Packet::alloc_outbound(1);
        anticheat_cyclelogic1_377(&mut packet, 0x1234, 0x56, true, 0x78);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[
                244, 15, 219, 0x93, 0x71, 61, 0xaa, 0xec, 0x12, 0x34, 0x56, 0xc7, 0xe3, 0x3d, 0xc0,
                97, 0x78
            ]
        );
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC1.length, -1);

        packet.pos = 0;
        anticheat_cyclelogic1_377(&mut packet, 0, 0, false, 0);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[244, 13, 219, 0x93, 0x71, 61, 0xaa, 0xec, 0, 0, 0, 0xc7, 0xe3, 97, 0]
        );

        packet.pos = 0;
        anticheat_cyclelogic3_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[197, 0, 0, 0, 0]);
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC3.length, 4);

        packet.pos = 0;
        anticheat_cyclelogic6_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[22, 0x95, 0xa0]);
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC6.length, 2);
        packet.pos = 0;
        anticheat_cyclelogic2_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[168]);
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC2.length, 0);

        packet.pos = 0;
        anticheat_cyclelogic4_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[173, 0x28, 0x8b, 0x80]);
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC4.length, 3);

        packet.pos = 0;
        anticheat_cyclelogic5_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[248]);
        assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC5.length, 0);
    }
}
