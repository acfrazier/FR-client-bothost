// Port of `~/experiments/Server/webclient/src/dash3d/ClientProj.ts`.
// `Client.loopCycle` moves in as parameters; `SpotType.list[id]` is looked up
// through the `Cache`.
use crate::config::Cache;
use crate::dash3d::{AnimFrame, Model};
use crate::datastruct::linkable::{LinkableTrait, Links};

pub struct ClientProj {
    pub links: Links,
    pub spotanim: i32,
    pub level: i32,
    pub src_x: i32,
    pub src_z: i32,
    pub h1: i32,
    pub h2: i32,
    pub t1: i32,
    pub t2: i32,
    pub angle: i32,
    pub startpos: i32,
    pub target: i32,
    pub mobile: bool,
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub velocity_x: f64,
    pub velocity_z: f64,
    pub velocity: f64,
    pub velocity_y: f64,
    pub acceleration_y: f64,
    pub yaw: i32,
    pub pitch: i32,
    pub anim_frame: i32,
    pub anim_cycle: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl ClientProj {
    pub fn new(
        spotanim: i32,
        level: i32,
        src_x: i32,
        h1: i32,
        src_z: i32,
        t1: i32,
        t2: i32,
        angle: i32,
        startpos: i32,
        target: i32,
        h2: i32,
    ) -> Self {
        ClientProj {
            links: Links::new(0),
            spotanim,
            level,
            src_x,
            src_z,
            h1,
            h2,
            t1,
            t2,
            angle,
            startpos,
            target,
            mobile: false,
            x: 0.0,
            z: 0.0,
            y: 0.0,
            velocity_x: 0.0,
            velocity_z: 0.0,
            velocity: 0.0,
            velocity_y: 0.0,
            acceleration_y: 0.0,
            yaw: 0,
            pitch: 0,
            anim_frame: 0,
            anim_cycle: 0,
            min_y: 1000,
        }
    }

    /// `setTarget(dstX, dstY, dstZ, cycle)` from client-ts.
    pub fn set_target(&mut self, dst_x: f64, dst_y: f64, dst_z: f64, cycle: i32) {
        if !self.mobile {
            let dx = dst_x - self.src_x as f64;
            let dz = dst_z - self.src_z as f64;
            let d = (dx * dx + dz * dz).sqrt();

            self.x = self.src_x as f64 + (dx * self.startpos as f64) / d;
            self.z = self.src_z as f64 + (dz * self.startpos as f64) / d;
            self.y = self.h1 as f64;
        }

        let dt = (self.t2 + 1 - cycle) as f64;
        self.velocity_x = (dst_x - self.x) / dt;
        self.velocity_z = (dst_z - self.z) / dt;
        self.velocity = (self.velocity_x * self.velocity_x + self.velocity_z * self.velocity_z).sqrt();
        if !self.mobile {
            self.velocity_y = -self.velocity * (self.angle as f64 * 0.02454369).tan();
        }
        self.acceleration_y = ((dst_y - self.y - self.velocity_y * dt) * 2.0) / (dt * dt);
    }

    /// `move(delta)` from client-ts (`move` is a Rust keyword).
    pub fn move_by(&mut self, cache: &Cache, delta: i32) {
        self.mobile = true;
        self.x += self.velocity_x * delta as f64;
        self.z += self.velocity_z * delta as f64;
        self.y += self.velocity_y * delta as f64
            + self.acceleration_y * 0.5 * delta as f64 * delta as f64;
        self.velocity_y += self.acceleration_y * delta as f64;
        self.yaw = ((f64::atan2(self.velocity_x, self.velocity_z) * 325.949 + 1024.0) as i32)
            & 0x7ff;
        self.pitch = ((f64::atan2(self.velocity_y, self.velocity) * 325.949) as i32) & 0x7ff;

        if let Some(seq) = cache.spot(self.spotanim as usize).seq {
            let seq_type = cache.seq(seq);
            self.anim_cycle += delta;

            while self.anim_cycle > seq_type.get_delay(self.anim_frame) {
                self.anim_cycle -= seq_type.get_delay(self.anim_frame) + 1;
                self.anim_frame += 1;
                if self.anim_frame >= seq_type.num_frames {
                    self.anim_frame = 0;
                }
            }
        }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache) -> Option<Model> {
        let spot = cache.spot(self.spotanim as usize);
        let spot_model = spot.get_temp_model2(cache)?;

        let mut frame = -1;
        if let Some(seq) = spot.seq {
            if let Some(frames) = &cache.seq(seq).frames {
                frame = frames.get(self.anim_frame as usize).copied().unwrap_or(-1);
            }
        }

        let mut model =
            Model::copy_for_anim(&spot_model, true, AnimFrame::animate_transparencies(frame), false);

        if frame != -1 {
            model.prepare_anim();
            model.animate(frame);
            model.label_faces = None;
            model.label_vertices = None;
        }

        if spot.resizeh != 128 || spot.resizev != 128 {
            model.resize(spot.resizeh, spot.resizev, spot.resizeh);
        }

        model.rotate_x_axis(self.pitch);
        model.calculate_normals(64 + spot.ambient, 850 + spot.contrast, -30, -50, -30, true);
        Some(model)
    }
}

impl LinkableTrait for ClientProj {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    }
}
