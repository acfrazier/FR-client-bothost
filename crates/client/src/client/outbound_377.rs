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
    /// Java `OPOBJ2` counts every action, including a failed approach.
    ///
    /// Java: `Client.java:10333-10353`.
    pub(crate) fn opobj2_packet_377(&mut self, object: i32, x: i32, z: i32) {
        self.oplogic1 += 1;
        if self.oplogic1 >= 120 {
            self.out.p1_enc(ClientProt377::ANTICHEAT_OPLOGIC1.id);
            self.out.p4(0);
            self.oplogic1 = 0;
        }
        self.out.p1_enc(ClientProt377::OPOBJ2.id);
        self.out.p2(x);
        p2_alt2(&mut self.out, z);
        p2_alt3(&mut self.out, object);
    }

    /// Java `OPHELD4` counts actions, not the legacy location counter.
    ///
    /// Java: `Client.java:9696-9708`.
    pub(crate) fn opheld4_packet_377(&mut self, object: i32, slot: i32, component: i32) {
        self.oplogic2 += 1;
        if self.oplogic2 >= 62 {
            self.out.p1_enc(ClientProt377::ANTICHEAT_OPLOGIC2.id);
            self.out.p1(206);
            self.oplogic2 = 0;
        }
        self.out.p1_enc(ClientProt377::OPHELD4.id);
        p2_alt1(&mut self.out, slot);
        p2_alt2(&mut self.out, object);
        self.out.p2(component);
    }

    /// Java `OPNPC3` sums NPC slots after the entity guard and approach attempt.
    ///
    /// Java: `Client.java:9960-9978`.
    pub(crate) fn opnpc3_packet_377(&mut self, npc_index: i32) {
        self.oplogic3 += npc_index;
        if self.oplogic3 >= 143 {
            self.out.p1_enc(ClientProt377::ANTICHEAT_OPLOGIC3.id);
            self.out.p4(0);
            self.oplogic3 = 0;
        }
        opnpc3_377(&mut self.out, npc_index);
    }

    /// Java `OPHELD1` sums object IDs, not inventory slots or components.
    ///
    /// Java: `Client.java:9746-9758`.
    pub(crate) fn opheld1_packet_377(&mut self, object: i32, slot: i32, component: i32) {
        self.oplogic5 += object;
        if self.oplogic5 >= 115 {
            self.out.p1_enc(ClientProt377::ANTICHEAT_OPLOGIC5.id);
            self.out.p1(125);
            self.oplogic5 = 0;
        }
        self.out.p1_enc(ClientProt377::OPHELD1.id);
        p2_alt2(&mut self.out, component);
        p2_alt1(&mut self.out, slot);
        p2_alt1(&mut self.out, object);
    }

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
    /// Reset the Java input fields and menu counters after cold login acceptance.
    ///
    /// Java: `Client.java:2742-2746,2761-2762,2832-2841`.
    pub(crate) fn cold_login_input_377(&mut self) {
        self.outbound.r377_mut().last_click_time = 0;
        self.outbound.r377_mut().duplicates = 0;
        self.outbound.r377_mut().focus = true;
        self.shell.focused = true;
        self.shell.idle_cycles = 0;
        if let Some(shared) = &self.shell.mouse_samples {
            shared.lock().unwrap().samples.clear();
        }
        self.oplogic1 = 0;
        self.oplogic2 = 0;
        self.oplogic3 = 0;
        self.oplogic4 = 0;
        self.oplogic5 = 0;
        self.oplogic6 = 0;
        self.oplogic7 = 0;
        self.oplogic8 = 0;
        self.oplogic9 = 0;
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
/// Emit Java `OPLOC2`: location id, scene x, then transformed scene z.
///
/// Java: `Client.java:10386-10392`.
pub(crate) fn oploc2_377(packet: &mut Packet, x: i32, z: i32, location: i32) {
    packet.p1_enc(ClientProt377::OPLOC2.id);
    packet.p2(location);
    packet.p2(x);
    p2_alt2(packet, z);
}
/// Emit Java `OPOBJ3`: transformed object type, transformed scene x, then
/// transformed scene z.
///
/// Java: `Client.java:10065-10070`.
pub(crate) fn opobj3_377(packet: &mut Packet, object_type: i32, x: i32, z: i32) {
    packet.p1_enc(ClientProt377::OPOBJ3.id);
    p2_alt3(packet, object_type);
    p2_alt3(packet, x);
    p2_alt2(packet, z);
}
/// Emit Java `ANTICHEAT_OPLOGIC4` before an OPOBJ3 at its threshold.
///
/// Java: `Client.java:10056-10064`.
pub(crate) fn anticheat_oplogic4_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::ANTICHEAT_OPLOGIC4.id);
    packet.p3(11257922);
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

/// Emit Java `OPNPC2` with its transformed NPC index.
///
/// Java: `Client.java:9733-9744`.
pub(crate) fn opnpc2_377(packet: &mut Packet, npc_index: i32) {
    packet.p1_enc(ClientProt377::OPNPC2.id);
    p2_alt2(packet, npc_index);
}
/// Emit Java `OPNPC3` with the transformed NPC index.
///
/// Java: `Client.java:9975-9978`.
pub(crate) fn opnpc3_377(packet: &mut Packet, npc_index: i32) {
    packet.p1_enc(ClientProt377::OPNPC3.id);
    p2_alt3(packet, npc_index);
}

/// Emit Java `OPNPCT`: transformed selected component, then transformed NPC index.
///
/// Java: `Client.java:10018-10030`.
pub(crate) fn opnpct_377(packet: &mut Packet, component_id: i32, npc_index: i32) {
    packet.p1_enc(ClientProt377::OPNPCT.id);
    p2_alt2(packet, component_id);
    p2_alt1(packet, npc_index);
}

/// Emit Java `OPHELD2` for one inventory object.
///
/// Java: `Client.java:9857-9873`.
pub(crate) fn opheld2_377(packet: &mut Packet, object_id: i32, slot: i32, component_id: i32) {
    packet.p1_enc(ClientProt377::OPHELD2.id);
    p2_alt1(packet, component_id);
    p2_alt1(packet, object_id);
    p2_alt2(packet, slot);
}
/// Emit Java `OPHELD5` for dropping one inventory object.
///
/// Java: `Client.java:10141-10147`.
pub(crate) fn opheld5_377(packet: &mut Packet, object_id: i32, slot: i32, component_id: i32) {
    packet.p1_enc(ClientProt377::OPHELD5.id);
    p2_alt1(packet, slot);
    p2_alt3(packet, object_id);
    p2_alt3(packet, component_id);
}

/// Emit Java `INV_BUTTON1`: transformed object id, component id, then slot.
///
/// Java: `Client.java:9780-9796`.
pub(crate) fn inv_button1_377(packet: &mut Packet, object_id: i32, slot: i32, component_id: i32) {
    packet.p1_enc(ClientProt377::INV_BUTTON1.id);
    p2_alt2(packet, object_id);
    packet.p2(component_id);
    packet.p2(slot);
}
/// Emit Java `INV_BUTTON2` for shop Buy-1 / Sell-1.
///
/// Java: `Client.java:10101-10107`.
pub(crate) fn inv_button2_377(packet: &mut Packet, object_id: i32, slot: i32, component_id: i32) {
    packet.p1_enc(ClientProt377::INV_BUTTON2.id);
    p2_alt2(packet, slot);
    p2_alt1(packet, object_id);
    p2_alt1(packet, component_id);
}

/// Emit Java `INV_BUTTON4` for the bank's Deposit-All / Withdraw-All option.
///
/// Java: `Client.java:10394-10410`.
pub(crate) fn inv_button4_377(packet: &mut Packet, object_id: i32, slot: i32, component_id: i32) {
    packet.p1_enc(ClientProt377::INV_BUTTON4.id);
    p2_alt3(packet, component_id);
    p2_alt1(packet, slot);
    packet.p2(object_id);
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

/// Emit Java's bodyless `CLOSE_MODAL` packet from `closeInterfaces`.
///
/// Java: `Client.java:4567-4596`.
pub(crate) fn close_modal_377(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::CLOSE_MODAL.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::{ClientConfig, ClientNpc, ClientPlayer, MiniMenuAction};
    use crate::dash3d::LocShape;
    use crate::io::{ClientProt, ClientRevision};

    fn client_with_revision(revision: ClientRevision) -> Client {
        Client::new_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: "/tmp".into(),
                members: true,
                lowmem: false,
            },
            revision,
        )
    }

    fn set_menu_action(client: &mut Client, action: i32, a: i32, b: i32, c: i32) {
        client.menu_action[0] = action;
        client.menu_param_a[0] = a;
        client.menu_param_b[0] = b;
        client.menu_param_c[0] = c;
    }

    fn install_ground_decor(client: &mut Client, location_id: i32, x: i32, z: i32) -> i32 {
        let typecode = location_id << 14;
        client
            .world
            .set_ground_decor(0, x, z, 0, typecode, LocShape::GROUND_DECOR, 0, 0, 0, 0);
        typecode
    }
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
            &[1, 0x12, 0x34, 0x45, 0x23, 0x56, 0x34, 0xe7, 0x45, 0x56, 0xf8, 0x67, 0x09]
        );
        assert_eq!(ClientProt377::OPHELDU.length, 12);
    }

    #[test]
    fn use_held_on_location_matches_java_377_literal() {
        let mut packet = Packet::alloc_outbound(1);
        oplocu_377(&mut packet, 0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789);

        assert_eq!(
            &packet.data()[..packet.pos],
            &[152, 0x34, 0x12, 0x45, 0x23, 0x56, 0x34, 0x67, 0x45, 0x56, 0x78, 0x09, 0x67]
        );
        assert_eq!(ClientProt377::OPLOCU.length, 12);
    }

    #[test]
    fn tutorial_interaction_writers_match_java_377_literal_bytes() {
        let mut packet = Packet::alloc_outbound(1);

        oploc2_377(&mut packet, 0x2345, 0x3456, 0x1234);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[241, 0x12, 0x34, 0x23, 0x45, 0x34, 0xd6]
        );
        assert_eq!(ClientProt377::OPLOC2.length, 6);

        packet.pos = 0;
        opnpc2_377(&mut packet, 0x1234);
        assert_eq!(&packet.data()[..packet.pos], &[67, 0x12, 0xb4]);
        assert_eq!(ClientProt377::OPNPC2.length, 2);

        packet.pos = 0;
        opheld2_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[24, 0x56, 0x34, 0x34, 0x12, 0x23, 0xc5]
        );
        assert_eq!(ClientProt377::OPHELD2.length, 6);

        packet.pos = 0;
        inv_button1_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[3, 0x12, 0xb4, 0x34, 0x56, 0x23, 0x45]
        );
        assert_eq!(ClientProt377::INV_BUTTON1.length, 6);

        packet.pos = 0;
        opnpct_377(&mut packet, 0x3456, 0x1234);
        assert_eq!(&packet.data()[..packet.pos], &[104, 0x34, 0xd6, 0x34, 0x12]);
        assert_eq!(ClientProt377::OPNPCT.length, 4);

        packet.pos = 0;
        if_button(&mut packet, 0x3456);
        assert_eq!(&packet.data()[..packet.pos], &[79, 0x34, 0x56]);
        assert_eq!(ClientProt377::IF_BUTTON.length, 2);

        packet.pos = 0;
        close_modal_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[110]);
        assert_eq!(ClientProt377::CLOSE_MODAL.length, 0);
    }

    #[test]
    fn do_action_oploc2_uses_377_layout_without_274_anticheat() {
        let mut client = client_with_revision(ClientRevision::R377);
        let typecode = install_ground_decor(&mut client, 0x1234, 5, 5);
        client.local_player = Some(ClientPlayer::at(5, 5));
        client.oplogic1 = 138;
        set_menu_action(&mut client, MiniMenuAction::OP_LOC2, typecode, 5, 5);

        client.doAction(0);

        let bytes = &client.out.data()[..client.out.pos];
        assert_eq!(
            &bytes[client.out.pos - 7..client.out.pos],
            &[241, 0x12, 0x34, 0, 5, 0, 0x85]
        );
        assert_eq!(client.oplogic1, 138);
        let legacy_anticheat = [ClientProt::ANTICHEAT_OPLOGIC1.id as u8, 0, 0, 0, 0];
        assert!(!bytes
            .windows(legacy_anticheat.len())
            .any(|w| w == legacy_anticheat));

        let mut legacy = client_with_revision(ClientRevision::R274);
        let typecode = install_ground_decor(&mut legacy, 0x1234, 5, 5);
        legacy.local_player = Some(ClientPlayer::at(5, 5));
        legacy.oplogic1 = 138;
        set_menu_action(&mut legacy, MiniMenuAction::OP_LOC2, typecode, 5, 5);

        legacy.doAction(0);

        let legacy_bytes = &legacy.out.data()[..legacy.out.pos];
        assert!(legacy_bytes
            .windows(5)
            .any(|w| w == [ClientProt::ANTICHEAT_OPLOGIC1.id as u8, 0, 0, 0, 0]));
        assert_eq!(
            &legacy_bytes[legacy.out.pos - 7..legacy.out.pos],
            &[ClientProt::OPLOC2.id as u8, 0, 5, 0, 5, 0x12, 0x34]
        );
    }

    #[test]
    fn do_action_npc2_and_spell_on_npc_keep_revision_specific_layouts() {
        for (revision, opcode, transformed_index) in [
            (ClientRevision::R377, 67, [0, 0xaa]),
            (ClientRevision::R274, ClientProt::OPNPC2.id as u8, [0, 42]),
        ] {
            let mut client = client_with_revision(revision);
            client.local_player = Some(ClientPlayer::at(5, 5));
            client.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
            set_menu_action(&mut client, MiniMenuAction::OP_NPC2, 42, 0, 0);

            client.doAction(0);

            assert_eq!(
                &client.out.data()[client.out.pos - 3..client.out.pos],
                &[opcode, transformed_index[0], transformed_index[1]]
            );
        }

        for (revision, expected) in [
            (ClientRevision::R377, [104, 0x34, 0xd6, 42, 0]),
            (
                ClientRevision::R274,
                [ClientProt::OPNPCT.id as u8, 0, 42, 0x34, 0x56],
            ),
        ] {
            let mut client = client_with_revision(revision);
            client.local_player = Some(ClientPlayer::at(5, 5));
            client.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
            client.target_com_id = 0x3456;
            set_menu_action(&mut client, MiniMenuAction::TGT_NPC, 42, 0, 0);

            client.doAction(0);

            assert_eq!(
                &client.out.data()[client.out.pos - expected.len()..client.out.pos],
                &expected
            );
        }
    }

    #[test]
    fn do_action_held2_and_inventory_button1_keep_274_isolation() {
        let mut client = client_with_revision(ClientRevision::R377);
        set_menu_action(
            &mut client,
            MiniMenuAction::OP_HELD2,
            0x1234,
            0x2345,
            0x3456,
        );
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[24, 0x56, 0x34, 0x34, 0x12, 0x23, 0xc5]
        );

        let mut legacy = client_with_revision(ClientRevision::R274);
        set_menu_action(
            &mut legacy,
            MiniMenuAction::OP_HELD2,
            0x1234,
            0x2345,
            0x3456,
        );
        legacy.doAction(0);
        assert_eq!(
            &legacy.out.data()[..legacy.out.pos],
            &[
                ClientProt::OPHELD2.id as u8,
                0x12,
                0x34,
                0x23,
                0x45,
                0x34,
                0x56
            ]
        );

        let mut client = client_with_revision(ClientRevision::R377);
        client.oplogic6 = 132;
        set_menu_action(
            &mut client,
            MiniMenuAction::INV_BUTTON1,
            0x1234,
            0x2345,
            0x3456,
        );
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[3, 0x12, 0xb4, 0x34, 0x56, 0x23, 0x45]
        );
        assert_eq!(client.oplogic6, 132);

        let mut legacy = client_with_revision(ClientRevision::R274);
        legacy.oplogic6 = 132;
        set_menu_action(
            &mut legacy,
            MiniMenuAction::INV_BUTTON1,
            0x1234,
            0x2345,
            0x3456,
        );
        legacy.doAction(0);
        assert_eq!(
            &legacy.out.data()[..legacy.out.pos],
            &[
                ClientProt::ANTICHEAT_OPLOGIC6.id as u8,
                0x17,
                0xe6,
                ClientProt::INV_BUTTON1.id as u8,
                0x12,
                0x34,
                0x23,
                0x45,
                0x34,
                0x56
            ]
        );
    }

    #[test]
    fn menu_anticheat_npc3_matches_java_threshold_and_resets() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.local_player = Some(ClientPlayer::at(5, 5));
        client.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
        client.oplogic3 = 100;
        set_menu_action(&mut client, MiniMenuAction::OP_NPC3, 42, 0, 0);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 13, 0xaa, 0]
        );
        assert_eq!(client.oplogic3, 142);

        client.out.pos = 0;
        client.npc[1] = Some(Box::new(ClientNpc::at(5, 5)));
        set_menu_action(&mut client, MiniMenuAction::OP_NPC3, 1, 0, 0);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 157, 0, 0, 0, 0, 13, 0x81, 0]
        );
        assert_eq!(client.oplogic3, 0);
        assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC3.length, 4);

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 13, 0x81, 0]
        );
        assert_eq!(client.oplogic3, 1);

        client.out.pos = 0;
        client.npc[3263] = Some(Box::new(ClientNpc::at(5, 5)));
        set_menu_action(&mut client, MiniMenuAction::OP_NPC3, 3263, 0, 0);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 157, 0, 0, 0, 0, 13, 0x3f, 0x0c]
        );
        assert_eq!(client.oplogic3, 0);

        client.out.pos = 0;
        client.oplogic3 = 142;
        client.npc[3263] = None;
        client.doAction(0);
        assert_eq!(client.out.pos, 0);
        assert_eq!(client.oplogic3, 142);
        assert!(client.outbound.r377_mut().unmapped.get().is_none());
    }

    #[test]
    fn menu_anticheat_obj2_matches_java_threshold_and_resets() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.local_player = Some(ClientPlayer::at(5, 5));
        client.oplogic1 = 118;
        set_menu_action(&mut client, MiniMenuAction::OP_OBJ2, 0x1234, 5, 5);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 100, 0, 5, 0, 0x85, 0xb4, 0x12]
        );
        assert_eq!(client.oplogic1, 119);

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 95, 0, 0, 0, 0, 100, 0, 5, 0, 0x85, 0xb4, 0x12]
        );
        assert_eq!(client.oplogic1, 0);
        assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC1.length, 4);
        assert_eq!(ClientProt377::OPOBJ2.length, 6);

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 100, 0, 5, 0, 0x85, 0xb4, 0x12]
        );
        assert_eq!(client.oplogic1, 1);
        assert!(client.outbound.r377_mut().unmapped.get().is_none());
    }

    #[test]
    fn menu_anticheat_held4_matches_java_threshold_and_resets() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.oplogic2 = 60;
        client.oplogic9 = 115;
        set_menu_action(
            &mut client,
            MiniMenuAction::OP_HELD4,
            0x1234,
            0x2345,
            0x3456,
        );
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[228, 0x45, 0x23, 0x12, 0xb4, 0x34, 0x56]
        );
        assert_eq!(client.oplogic2, 61);
        assert_eq!(client.oplogic9, 115);

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[165, 206, 228, 0x45, 0x23, 0x12, 0xb4, 0x34, 0x56]
        );
        assert_eq!(client.oplogic2, 0);
        assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC2.length, 1);
        assert_eq!(ClientProt377::OPHELD4.length, 6);
        assert_eq!(
            (
                client.selected_item,
                client.selected_com_id,
                client.selected_area
            ),
            (0x2345, 0x3456, 2)
        );

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[228, 0x45, 0x23, 0x12, 0xb4, 0x34, 0x56]
        );
        assert_eq!(client.oplogic2, 1);
        assert!(client.outbound.r377_mut().unmapped.get().is_none());
    }

    #[test]
    fn menu_anticheat_held1_matches_java_object_sum_and_resets() {
        let mut client = client_with_revision(ClientRevision::R377);
        set_menu_action(&mut client, MiniMenuAction::OP_HELD1, 57, 0x2345, 0x3456);
        for sum in [57, 114] {
            client.out.pos = 0;
            client.doAction(0);
            assert_eq!(
                &client.out.data()[..client.out.pos],
                &[203, 0x34, 0xd6, 0x45, 0x23, 57, 0]
            );
            assert_eq!(client.oplogic5, sum);
        }

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[126, 125, 203, 0x34, 0xd6, 0x45, 0x23, 57, 0]
        );
        assert_eq!(client.oplogic5, 0);
        assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC5.length, 1);
        assert_eq!(ClientProt377::OPHELD1.length, 6);

        client.out.pos = 0;
        client.oplogic5 = 114;
        set_menu_action(&mut client, MiniMenuAction::OP_HELD1, 1, 0x2345, 0x3456);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[126, 125, 203, 0x34, 0xd6, 0x45, 0x23, 1, 0]
        );
        assert_eq!(client.oplogic5, 0);

        client.out.pos = 0;
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[203, 0x34, 0xd6, 0x45, 0x23, 1, 0]
        );
        assert_eq!(client.oplogic5, 1);
        assert!(client.outbound.r377_mut().unmapped.get().is_none());
    }

    #[test]
    fn menu_anticheat_obj3_counts_only_divisible_object_ids() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.local_player = Some(ClientPlayer::at(5, 5));
        client.oplogic4 = 83;
        set_menu_action(&mut client, MiniMenuAction::OP_OBJ3, 0x1235, 5, 5);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[247, 5, 0x85, 0, 0, 0x85, 0, 71, 0xb5, 0x12, 0x85, 0, 0, 0x85]
        );
        assert_eq!(client.oplogic4, 83);

        client.out.pos = 0;
        set_menu_action(&mut client, MiniMenuAction::OP_OBJ3, 0x1234, 5, 5);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[
                247, 5, 0x85, 0, 0, 0x85, 0, 222, 0xab, 0xc8, 0x42, 71, 0xb4, 0x12, 0x85, 0, 0,
                0x85
            ]
        );
        assert_eq!(client.oplogic4, 0);
    }

    #[test]
    fn menu_anticheat_cold_login_resets_all_shared_counters() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.oplogic1 = 119;
        client.oplogic2 = 61;
        client.oplogic3 = 142;
        client.oplogic4 = 83;
        client.oplogic5 = 114;
        client.oplogic6 = 6;
        client.oplogic7 = 7;
        client.oplogic8 = 8;
        client.oplogic9 = 9;
        client.cold_login_input_377();
        assert_eq!(
            [
                client.oplogic1,
                client.oplogic2,
                client.oplogic3,
                client.oplogic4,
                client.oplogic5,
                client.oplogic6,
                client.oplogic7,
                client.oplogic8,
                client.oplogic9,
            ],
            [0; 9]
        );
    }

    #[test]
    fn new_menu_actions_route_to_377_and_preserve_legacy_layouts() {
        let mut client = client_with_revision(ClientRevision::R377);
        client.local_player = Some(ClientPlayer::at(5, 5));
        client.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
        set_menu_action(&mut client, MiniMenuAction::OP_NPC3, 42, 0, 0);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[client.out.pos - 3..client.out.pos],
            &[13, 0xaa, 0]
        );

        let mut client = client_with_revision(ClientRevision::R377);
        client.local_player = Some(ClientPlayer::at(5, 5));
        set_menu_action(&mut client, MiniMenuAction::OP_OBJ3, 0x1234, 5, 5);
        client.doAction(0);
        assert_eq!(
            &client.out.data()[client.out.pos - 7..client.out.pos],
            &[71, 0xb4, 0x12, 0x85, 0, 0, 0x85]
        );
        assert_eq!(client.oplogic4, 1);

        let mut threshold = client_with_revision(ClientRevision::R377);
        threshold.local_player = Some(ClientPlayer::at(5, 5));
        threshold.oplogic4 = 83;
        set_menu_action(&mut threshold, MiniMenuAction::OP_OBJ3, 0x1234, 5, 5);
        threshold.doAction(0);
        assert_eq!(
            &threshold.out.data()[threshold.out.pos - 11..threshold.out.pos],
            &[222, 0xab, 0xc8, 0x42, 71, 0xb4, 0x12, 0x85, 0, 0, 0x85]
        );
        assert_eq!(threshold.oplogic4, 0);

        let mut client = client_with_revision(ClientRevision::R377);
        set_menu_action(
            &mut client,
            MiniMenuAction::OP_HELD5,
            0x1234,
            0x2345,
            0x3456,
        );
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[4, 0x45, 0x23, 0xb4, 0x12, 0xd6, 0x34]
        );
        assert_eq!(
            (
                client.selected_item,
                client.selected_com_id,
                client.selected_area
            ),
            (0x2345, 0x3456, 2)
        );

        let mut client = client_with_revision(ClientRevision::R377);
        client.oplogic6 = 132;
        set_menu_action(
            &mut client,
            MiniMenuAction::INV_BUTTON2,
            0x1234,
            0x2345,
            0x3456,
        );
        client.doAction(0);
        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[177, 0x23, 0xc5, 0x34, 0x12, 0x56, 0x34]
        );
        assert_eq!(client.oplogic6, 132);
        assert_eq!(
            (
                client.selected_item,
                client.selected_com_id,
                client.selected_area
            ),
            (0x2345, 0x3456, 2)
        );

        for (revision, npc3, obj3, held5, inv2) in [
            (ClientRevision::R274, 223, 108, 42, 82),
            (ClientRevision::R289, 178, 110, 79, 111),
        ] {
            let mut client = client_with_revision(revision);
            client.local_player = Some(ClientPlayer::at(5, 5));
            client.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
            set_menu_action(&mut client, MiniMenuAction::OP_NPC3, 42, 0, 0);
            client.doAction(0);
            assert_eq!(
                &client.out.data()[client.out.pos - 3..client.out.pos],
                &[npc3, 0, 42]
            );

            let mut client = client_with_revision(revision);
            client.local_player = Some(ClientPlayer::at(5, 5));
            set_menu_action(&mut client, MiniMenuAction::OP_OBJ3, 0x1234, 5, 5);
            client.doAction(0);
            assert_eq!(
                &client.out.data()[client.out.pos - 7..client.out.pos],
                &[obj3, 0, 5, 0, 5, 0x12, 0x34]
            );
            assert_eq!(client.oplogic4, 0);

            let mut client = client_with_revision(revision);
            set_menu_action(
                &mut client,
                MiniMenuAction::OP_HELD5,
                0x1234,
                0x2345,
                0x3456,
            );
            client.doAction(0);
            assert_eq!(
                &client.out.data()[..client.out.pos],
                &[held5, 0x12, 0x34, 0x23, 0x45, 0x34, 0x56]
            );

            let mut client = client_with_revision(revision);
            set_menu_action(
                &mut client,
                MiniMenuAction::INV_BUTTON2,
                0x1234,
                0x2345,
                0x3456,
            );
            client.doAction(0);
            assert_eq!(
                &client.out.data()[..client.out.pos],
                &[inv2, 0x12, 0x34, 0x23, 0x45, 0x34, 0x56]
            );
        }
    }

    #[test]
    fn do_action_run_and_combat_buttons_and_close_use_revision_packet_ids() {
        for action in [MiniMenuAction::TOGGLE_BUTTON, MiniMenuAction::SELECT_BUTTON] {
            let mut client = client_with_revision(ClientRevision::R377);
            set_menu_action(&mut client, action, 0, 0, 0x1234);
            client.doAction(0);
            assert_eq!(&client.out.data()[..client.out.pos], &[79, 0x12, 0x34]);

            let mut legacy = client_with_revision(ClientRevision::R274);
            set_menu_action(&mut legacy, action, 0, 0, 0x1234);
            legacy.doAction(0);
            assert_eq!(
                &legacy.out.data()[..legacy.out.pos],
                &[ClientProt::IF_BUTTON.id as u8, 0x12, 0x34]
            );
        }

        let mut client = client_with_revision(ClientRevision::R377);
        client.main_modal_id = 17;
        set_menu_action(&mut client, MiniMenuAction::CLOSE_BUTTON, 0, 0, 0);
        client.doAction(0);
        assert_eq!(&client.out.data()[..client.out.pos], &[110]);
        assert_eq!(client.main_modal_id, -1);

        let mut legacy = client_with_revision(ClientRevision::R274);
        legacy.main_modal_id = 17;
        set_menu_action(&mut legacy, MiniMenuAction::CLOSE_BUTTON, 0, 0, 0);
        legacy.doAction(0);
        assert_eq!(
            &legacy.out.data()[..legacy.out.pos],
            &[ClientProt::CLOSE_MODAL.id as u8]
        );
        assert_eq!(legacy.main_modal_id, -1);
    }

    #[test]
    fn use_held_on_held_do_action_routes_377_and_marks_target() {
        use super::super::{ClientConfig, MiniMenuAction};
        use crate::io::ClientRevision;

        let mut client = Client::new_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: "/tmp".into(),
                members: true,
                lowmem: false,
            },
            ClientRevision::R377,
        );
        client.menu_action[0] = MiniMenuAction::USEHELD_ONHELD;
        client.menu_param_a[0] = 0x6789;
        client.menu_param_b[0] = 0x5678;
        client.menu_param_c[0] = 0x1234;
        client.obj_com_id = 0x3456;
        client.obj_selected_slot = 0x2345;
        client.obj_selected_com_id = 0x4567;
        client.use_mode = 1;
        client.target_mode = 1;

        client.doAction(0);

        assert_eq!(
            &client.out.data()[..client.out.pos],
            &[1, 0x67, 0x89, 0x45, 0x23, 0x56, 0x34, 0xe7, 0x45, 0x56, 0xf8, 0x12, 0xb4]
        );
        assert_eq!(client.selected_cycle, 0);
        assert_eq!(client.selected_com_id, 0x1234);
        assert_eq!(client.selected_item, 0x5678);
        assert_eq!(client.selected_area, 2);
        assert_eq!((client.use_mode, client.target_mode), (0, 0));
    }

    #[test]
    fn use_held_on_held_keeps_274_wire_path() {
        use super::super::{ClientConfig, MiniMenuAction};
        use crate::io::ClientRevision;

        let mut client = Client::new_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: "/tmp".into(),
                members: true,
                lowmem: false,
            },
            ClientRevision::R274,
        );
        client.menu_action[0] = MiniMenuAction::USEHELD_ONHELD;
        client.menu_param_a[0] = 0x6789;
        client.menu_param_b[0] = 0x5678;
        client.menu_param_c[0] = 0x1234;
        client.obj_com_id = 0x3456;
        client.obj_selected_slot = 0x2345;
        client.obj_selected_com_id = 0x4567;

        client.doAction(0);

        assert_eq!(
            &client.out.data()[1..client.out.pos],
            &[0x67, 0x89, 0x56, 0x78, 0x12, 0x34, 0x34, 0x56, 0x23, 0x45, 0x45, 0x67]
        );
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
    fn new_377_action_writers_match_java_packet_literals() {
        // The byte transforms are Java Packet.p2_alt1/2/3
        // (Packet.java:340-354), independently checked against each action.
        let mut packet = Packet::alloc_outbound(1);
        anticheat_oplogic4_377(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[222, 0xab, 0xc8, 0x42]);
        assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC4.length, 3);
        packet.pos = 0;

        opnpc3_377(&mut packet, 0x1234);
        assert_eq!(&packet.data()[..packet.pos], &[13, 0xb4, 0x12]);
        assert_eq!(ClientProt377::OPNPC3.length, 2);

        packet.pos = 0;
        opobj3_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[71, 0xb4, 0x12, 0xc5, 0x23, 0x34, 0xd6]
        );
        assert_eq!(ClientProt377::OPOBJ3.length, 6);

        packet.pos = 0;
        opheld5_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[4, 0x45, 0x23, 0xb4, 0x12, 0xd6, 0x34]
        );
        assert_eq!(ClientProt377::OPHELD5.length, 6);

        packet.pos = 0;
        inv_button2_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[177, 0x23, 0xc5, 0x34, 0x12, 0x56, 0x34]
        );
        assert_eq!(ClientProt377::INV_BUTTON2.length, 6);
    }

    #[test]
    fn bank_all_writer_matches_java_literal_bytes() {
        let mut packet = Packet::alloc_outbound(1);
        inv_button4_377(&mut packet, 0x1234, 0x2345, 0x3456);
        assert_eq!(
            &packet.data()[..packet.pos],
            &[231, 0xd6, 0x34, 0x45, 0x23, 0x12, 0x34]
        );
        assert_eq!(ClientProt377::INV_BUTTON4.length, 6);
    }

    #[test]
    fn bank_all_action_routes_377_and_preserves_legacy_layouts() {
        for (revision, expected) in [
            (
                ClientRevision::R377,
                [231, 0xd6, 0x34, 0x45, 0x23, 0x12, 0x34],
            ),
            (
                ClientRevision::R274,
                [
                    ClientProt::INV_BUTTON4.id as u8,
                    0x12,
                    0x34,
                    0x23,
                    0x45,
                    0x34,
                    0x56,
                ],
            ),
            (
                ClientRevision::R289,
                [248, 0x12, 0x34, 0x23, 0x45, 0x34, 0x56],
            ),
        ] {
            let mut client = client_with_revision(revision);
            set_menu_action(
                &mut client,
                MiniMenuAction::INV_BUTTON4,
                0x1234,
                0x2345,
                0x3456,
            );
            client.doAction(0);
            assert_eq!(&client.out.data()[..client.out.pos], &expected);
            assert_eq!(
                (
                    client.selected_item,
                    client.selected_com_id,
                    client.selected_area
                ),
                (0x2345, 0x3456, 2)
            );
        }
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
