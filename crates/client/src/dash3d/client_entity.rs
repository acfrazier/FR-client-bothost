// Port of `~/experiments/Server/webclient/src/dash3d/ClientEntity.ts`. The
// TS abstract `isReady` is a per-subclass method in this port. `teleport` and
// `moveCode` take the config `Cache` because the `SeqType.list` static moved
// onto the `Client`.
use crate::config::seq_type::{
    POSTANIM_ABORTANIM, POSTANIM_DELAYMOVE, PREANIM_DELAYANIM, PREANIM_DELAYMOVE,
};
use crate::config::Cache;

const ROUTE_CAPACITY: usize = 10;

#[derive(Clone)]
pub struct ClientEntity {
    pub x: i32,
    pub z: i32,
    pub yaw: i32,
    pub needs_forward_draw_padding: bool,
    pub size: i32,
    pub readyanim: i32,
    pub turnanim: i32,
    pub walkanim: i32,
    pub walkanim_b: i32,
    pub walkanim_l: i32,
    pub walkanim_r: i32,
    pub runanim: i32,
    pub chat_message: Option<String>,
    pub chat_timer: i32,
    pub chat_colour: i32,
    pub chat_effect: i32,
    pub combat_cycle: i32,
    pub damage_values: [i32; 4],
    pub damage_types: [i32; 4],
    pub damage_cycles: [i32; 4],
    pub health: i32,
    pub total_health: i32,
    pub face_entity: i32,
    pub face_square_x: i32,
    pub face_square_z: i32,
    pub secondary_anim: i32,
    pub secondary_anim_frame: i32,
    pub secondary_anim_cycle: i32,
    pub primary_anim: i32,
    pub primary_anim_frame: i32,
    pub primary_anim_cycle: i32,
    pub primary_anim_delay: i32,
    pub primary_anim_loop: i32,
    pub spotanim_id: i32,
    pub spotanim_frame: i32,
    pub spotanim_cycle: i32,
    pub spotanim_last_cycle: i32,
    pub spotanim_height: i32,
    pub exact_start_x: i32,
    pub exact_end_x: i32,
    pub exact_start_z: i32,
    pub exact_end_z: i32,
    pub exact_move_end: i32,
    pub exact_move_start: i32,
    pub exact_move_facing: i32,
    pub cycle: i32,
    pub height: i32,
    pub dst_yaw: i32,
    pub route_length: i32,
    pub route_x: Vec<i32>,
    pub route_z: Vec<i32>,
    pub route_run: Vec<bool>,
    pub anim_delay_move: i32,
    pub preanim_route_length: i32,
    pub turnspeed: i32,
}

impl Default for ClientEntity {
    fn default() -> Self {
        ClientEntity {
            x: 0,
            z: 0,
            yaw: 0,
            needs_forward_draw_padding: false,
            size: 1,
            readyanim: -1,
            turnanim: -1,
            walkanim: -1,
            walkanim_b: -1,
            walkanim_l: -1,
            walkanim_r: -1,
            runanim: -1,
            chat_message: None,
            chat_timer: 100,
            chat_colour: 0,
            chat_effect: 0,
            combat_cycle: -1000,
            damage_values: [0; 4],
            damage_types: [0; 4],
            damage_cycles: [0; 4],
            health: 0,
            total_health: 0,
            face_entity: -1,
            face_square_x: 0,
            face_square_z: 0,
            secondary_anim: -1,
            secondary_anim_frame: 0,
            secondary_anim_cycle: 0,
            primary_anim: -1,
            primary_anim_frame: 0,
            primary_anim_cycle: 0,
            primary_anim_delay: 0,
            primary_anim_loop: 0,
            spotanim_id: -1,
            spotanim_frame: 0,
            spotanim_cycle: 0,
            spotanim_last_cycle: 0,
            spotanim_height: 0,
            exact_start_x: 0,
            exact_end_x: 0,
            exact_start_z: 0,
            exact_end_z: 0,
            exact_move_end: 0,
            exact_move_start: 0,
            exact_move_facing: 0,
            cycle: 0,
            height: 0,
            dst_yaw: 0,
            route_length: 0,
            route_x: vec![0; ROUTE_CAPACITY],
            route_z: vec![0; ROUTE_CAPACITY],
            route_run: vec![false; ROUTE_CAPACITY],
            anim_delay_move: 0,
            preanim_route_length: 0,
            turnspeed: 32,
        }
    }
}

impl ClientEntity {
    /// Test/headless constructor: a fresh entity standing on the given tile.
    pub fn at(x: i32, z: i32) -> Self {
        let mut entity = ClientEntity::default();
        entity.route_x[0] = x;
        entity.route_z[0] = z;
        entity
    }

    /// `teleport(jump, x, z)` from client-ts.
    pub fn teleport(&mut self, cache: &Cache, jump: bool, x: i32, z: i32) {
        if self.primary_anim != -1
            && cache.seq(self.primary_anim as usize).postanim_move == POSTANIM_ABORTANIM
        {
            self.primary_anim = -1;
        }

        if !jump {
            let dx = x - self.route_x[0];
            let dz = z - self.route_z[0];

            if dx >= -8 && dx <= 8 && dz >= -8 && dz <= 8 {
                if self.route_length < 9 {
                    self.route_length += 1;
                }

                for i in (1..=self.route_length as usize).rev() {
                    self.route_x[i] = self.route_x[i - 1];
                    self.route_z[i] = self.route_z[i - 1];
                    self.route_run[i] = self.route_run[i - 1];
                }

                self.route_x[0] = x;
                self.route_z[0] = z;
                self.route_run[0] = false;
                return;
            }
        }

        self.route_length = 0;
        self.preanim_route_length = 0;
        self.anim_delay_move = 0;
        self.route_x[0] = x;
        self.route_z[0] = z;
        self.x = self.route_x[0] * 128 + self.size * 64;
        self.z = self.route_z[0] * 128 + self.size * 64;
    }

    /// `moveCode(running, direction)` from client-ts.
    pub fn move_code(&mut self, cache: &Cache, running: bool, direction: i32) {
        let mut next_x = self.route_x[0];
        let mut next_z = self.route_z[0];

        match direction {
            0 => {
                next_x -= 1;
                next_z += 1;
            }
            1 => next_z += 1,
            2 => {
                next_x += 1;
                next_z += 1;
            }
            3 => next_x -= 1,
            4 => next_x += 1,
            5 => {
                next_x -= 1;
                next_z -= 1;
            }
            6 => next_z -= 1,
            7 => {
                next_x += 1;
                next_z -= 1;
            }
            _ => {}
        }

        if self.primary_anim != -1
            && cache.seq(self.primary_anim as usize).postanim_move == POSTANIM_ABORTANIM
        {
            self.primary_anim = -1;
        }

        if self.route_length < 9 {
            self.route_length += 1;
        }

        for i in (1..=self.route_length as usize).rev() {
            self.route_x[i] = self.route_x[i - 1];
            self.route_z[i] = self.route_z[i - 1];
            self.route_run[i] = self.route_run[i - 1];
        }

        self.route_x[0] = next_x;
        self.route_z[0] = next_z;
        self.route_run[0] = running;
    }

    /// `abortRoute()` from client-ts.
    pub fn abort_route(&mut self) {
        self.route_length = 0;
        self.preanim_route_length = 0;
    }

    /// `routeMove(e)` from Java (`Client.java` 10639): advance one 20 ms
    /// step toward the route head `route[routeLength - 1]`. Speed is 4 (2
    /// while turning), 6/8 for longer routes, doubled when the head segment
    /// is a run; the walk/run anim id is picked from the yaw-delta octants.
    pub fn route_move(&mut self, cache: &Cache) {
        self.secondary_anim = self.readyanim;
        if self.route_length == 0 {
            self.anim_delay_move = 0;
            return;
        }
        if self.primary_anim != -1 && self.primary_anim_delay == 0 {
            let seq = cache.seq(self.primary_anim as usize);
            if self.preanim_route_length > 0 && seq.preanim_move == PREANIM_DELAYMOVE {
                self.anim_delay_move += 1;
                return;
            }
            if self.preanim_route_length <= 0 && seq.postanim_move == POSTANIM_DELAYMOVE {
                self.anim_delay_move += 1;
                return;
            }
        }
        let x = self.x;
        let z = self.z;
        let dest_x = self.route_x[self.route_length as usize - 1] * 128 + self.size * 64;
        let dest_z = self.route_z[self.route_length as usize - 1] * 128 + self.size * 64;
        if dest_x - x > 256 || dest_x - x < -256 || dest_z - z > 256 || dest_z - z < -256 {
            self.x = dest_x;
            self.z = dest_z;
            return;
        }
        if x < dest_x {
            if z < dest_z {
                self.dst_yaw = 1280;
            } else if z > dest_z {
                self.dst_yaw = 1792;
            } else {
                self.dst_yaw = 1536;
            }
        } else if x > dest_x {
            if z < dest_z {
                self.dst_yaw = 768;
            } else if z > dest_z {
                self.dst_yaw = 256;
            } else {
                self.dst_yaw = 512;
            }
        } else if z < dest_z {
            self.dst_yaw = 1024;
        } else {
            self.dst_yaw = 0;
        }
        let mut delta_yaw = (self.dst_yaw - self.yaw) & 0x7ff;
        if delta_yaw > 1024 {
            delta_yaw -= 2048;
        }
        let mut seq_id = self.walkanim_b;
        if delta_yaw >= -256 && delta_yaw <= 256 {
            seq_id = self.walkanim;
        } else if delta_yaw >= 256 && delta_yaw < 768 {
            seq_id = self.walkanim_l;
        } else if delta_yaw >= -768 && delta_yaw <= -256 {
            seq_id = self.walkanim_r;
        }
        if seq_id == -1 {
            seq_id = self.walkanim;
        }
        self.secondary_anim = seq_id;
        let mut move_speed = 4;
        if self.yaw != self.dst_yaw && self.face_entity == -1 && self.turnspeed != 0 {
            move_speed = 2;
        }
        if self.route_length > 2 {
            move_speed = 6;
        }
        if self.route_length > 3 {
            move_speed = 8;
        }
        if self.anim_delay_move > 0 && self.route_length > 1 {
            move_speed = 8;
            self.anim_delay_move -= 1;
        }
        if self.route_run[self.route_length as usize - 1] {
            move_speed <<= 1;
        }
        if move_speed >= 8 && self.secondary_anim == self.walkanim && self.runanim != -1 {
            self.secondary_anim = self.runanim;
        }
        if x < dest_x {
            self.x += move_speed;
            if self.x > dest_x {
                self.x = dest_x;
            }
        } else if x > dest_x {
            self.x -= move_speed;
            if self.x < dest_x {
                self.x = dest_x;
            }
        }
        if z < dest_z {
            self.z += move_speed;
            if self.z > dest_z {
                self.z = dest_z;
            }
        } else if z > dest_z {
            self.z -= move_speed;
            if self.z < dest_z {
                self.z = dest_z;
            }
        }
        if self.x == dest_x && self.z == dest_z {
            self.route_length -= 1;
            if self.preanim_route_length > 0 {
                self.preanim_route_length -= 1;
            }
        }
    }

    /// `entityAnim(e)` from Java (`Client.java` 10813): advance the
    /// secondary (walk/run), spotanim, and primary animation frame cycles.
    /// The primary `preanim_move == DELAYANIM` hold keeps the pose during
    /// an exact move.
    pub fn entity_anim(&mut self, cache: &Cache, loop_cycle: i32) {
        self.needs_forward_draw_padding = false;
        if self.secondary_anim != -1 {
            let seq = cache.seq(self.secondary_anim as usize);
            self.secondary_anim_cycle += 1;
            if self.secondary_anim_frame < seq.num_frames
                && self.secondary_anim_cycle > seq.get_delay(self.secondary_anim_frame)
            {
                self.secondary_anim_cycle = 0;
                self.secondary_anim_frame += 1;
            }
            if self.secondary_anim_frame >= seq.num_frames {
                self.secondary_anim_cycle = 0;
                self.secondary_anim_frame = 0;
            }
        }
        if self.spotanim_id != -1 && loop_cycle >= self.spotanim_last_cycle {
            if self.spotanim_frame < 0 {
                self.spotanim_frame = 0;
            }
            if let Some(seq_id) = cache.spot(self.spotanim_id as usize).seq {
                let seq = cache.seq(seq_id);
                self.spotanim_cycle += 1;
                while self.spotanim_frame < seq.num_frames
                    && self.spotanim_cycle > seq.get_delay(self.spotanim_frame)
                {
                    self.spotanim_cycle -= seq.get_delay(self.spotanim_frame);
                    self.spotanim_frame += 1;
                }
                if self.spotanim_frame >= seq.num_frames
                    && (self.spotanim_frame < 0 || self.spotanim_frame >= seq.num_frames)
                {
                    self.spotanim_id = -1;
                }
            }
        }
        if self.primary_anim != -1 && self.primary_anim_delay <= 1 {
            let seq = cache.seq(self.primary_anim as usize);
            if seq.preanim_move == PREANIM_DELAYANIM
                && self.preanim_route_length > 0
                && self.exact_move_start <= loop_cycle
                && self.exact_move_end < loop_cycle
            {
                self.primary_anim_delay = 1;
                return;
            }
        }
        if self.primary_anim != -1 && self.primary_anim_delay == 0 {
            let seq = cache.seq(self.primary_anim as usize);
            self.primary_anim_cycle += 1;
            while self.primary_anim_frame < seq.num_frames
                && self.primary_anim_cycle > seq.get_delay(self.primary_anim_frame)
            {
                self.primary_anim_cycle -= seq.get_delay(self.primary_anim_frame);
                self.primary_anim_frame += 1;
            }
            if self.primary_anim_frame >= seq.num_frames {
                self.primary_anim_frame -= seq.loops;
                self.primary_anim_loop += 1;
                if self.primary_anim_loop >= seq.maxloops {
                    self.primary_anim = -1;
                }
                if self.primary_anim_frame < 0 || self.primary_anim_frame >= seq.num_frames {
                    self.primary_anim = -1;
                }
            }
            self.needs_forward_draw_padding = seq.reachforward;
        }
        if self.primary_anim_delay > 0 {
            self.primary_anim_delay -= 1;
        }
    }

    /// `addHitmark(loopCycle, type, value)` from client-ts.
    pub fn add_hitmark(&mut self, loop_cycle: i32, r#type: i32, value: i32) {
        for i in 0..4 {
            if self.damage_cycles[i] <= loop_cycle {
                self.damage_values[i] = value;
                self.damage_types[i] = r#type;
                self.damage_cycles[i] = loop_cycle + 70;
                return;
            }
        }
    }
}
