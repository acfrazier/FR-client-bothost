//! Primary 289 input telemetry; no server policy or renderer ownership.
use super::client::Client;
use crate::io::ClientProt;

#[derive(Default)]
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

    fn mouse_packet_289(&mut self) {
        let opcode = self.client_opcode(ClientProt::EVENT_MOUSE_MOVE);
        let Some(shared) = &self.shell.mouse_samples else {
            return;
        };
        let mut recorder = shared.lock().unwrap();
        if !self.mouse_tracked {
            recorder.samples.clear();
            return;
        }
        if self.shell.mouse_click_button == 0 && recorder.samples.len() < 40 {
            return;
        }
        self.out.p1_enc(opcode);
        self.out.p1(0);
        let start = self.out.pos;
        let state = &mut self.outbound_289;
        // Authorized correction to J:5829's reversed cursor subtraction:
        // test the forward budget before each whole sample, retaining leftovers.
        while self.out.pos - start < 240 {
            let Some((raw_x, raw_y)) = recorder.samples.pop_front() else {
                break;
            };
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
                self.out
                    .p2((state.duplicates << 12) + ((dx + 32) << 6) + dy + 32);
            } else if state.duplicates < 8 {
                self.out.p3((state.duplicates << 19) + coordinate + 8388608);
            } else {
                self.out
                    .p4((state.duplicates << 19) + coordinate - 1073741824);
            }
            state.duplicates = 0;
        }
        self.out.psize1((self.out.pos - start) as i32);
    }

    pub(crate) fn input_packets_289(&mut self) {
        self.mouse_packet_289();
        // J:5882-5907: snapshot before drag/tutorial/menu consume the click.
        if self.shell.mouse_click_button != 0 {
            let elapsed = ((self.shell.mouse_click_time - self.outbound_289.last_click_time) / 50)
                .min(4095) as i32;
            self.outbound_289.last_click_time = self.shell.mouse_click_time;
            let coordinate = self.shell.mouse_click_y.clamp(0, 502) * 765
                + self.shell.mouse_click_x.clamp(0, 764);
            let button = i32::from(self.shell.mouse_click_button == 2);
            self.out
                .p1_enc(self.client_opcode(ClientProt::EVENT_MOUSE_CLICK));
            self.out.p4((elapsed << 20) + (button << 19) + coordinate);
        }
        let state = &mut self.outbound_289;
        state.camera_delay = state.camera_delay.saturating_sub(1);
        if self.shell.key_held[1..=4].contains(&1) {
            state.camera_pending = true;
        }
        // J:5909-5920: input is latched while rate-limited; pitch then yaw.
        if state.camera_pending && state.camera_delay == 0 {
            state.camera_delay = 20;
            state.camera_pending = false;
            self.out
                .p1_enc(self.client_opcode(ClientProt::EVENT_CAMERA_POSITION));
            self.out.p2(self.orbit_camera_pitch);
            self.out.p2(self.orbit_camera_yaw);
        }
        if self.outbound_289.blurred == self.shell.focused {
            self.outbound_289.blurred = !self.shell.focused;
            self.out
                .p1_enc(self.client_opcode(ClientProt::EVENT_APPLET_FOCUS));
            self.out.p1(i32::from(self.shell.focused));
        }
    }
}
