// Port of `~/experiments/Server/webclient/src/dash3d/ClientLocAnim.ts`.
// `Client.loopCycle` moves in as a parameter; `SeqType.list[seq]` is looked
// up through the `Cache`.
use crate::config::Cache;
use crate::dash3d::Model;

pub struct ClientLocAnim {
    pub index: i32,
    pub shape: i32,
    pub angle: i32,
    pub height_sw: i32,
    pub height_se: i32,
    pub height_ne: i32,
    pub height_nw: i32,
    pub anim: Option<usize>,
    pub anim_frame: i32,
    pub anim_cycle: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl ClientLocAnim {
    pub fn new(
        cache: &Cache,
        index: i32,
        shape: i32,
        angle: i32,
        height_sw: i32,
        height_se: i32,
        height_ne: i32,
        height_nw: i32,
        seq: usize,
        random_frame: bool,
        loop_cycle: i32,
    ) -> Self {
        let seq_type = cache.seq(seq);
        let mut anim_cycle = loop_cycle;
        let mut anim_frame = 0;

        if random_frame && seq_type.loops != -1 {
            anim_frame = (random_float() * seq_type.num_frames as f64) as i32;
            anim_cycle -= (random_float() * seq_type.get_delay(anim_frame) as f64) as i32;
        }

        ClientLocAnim {
            index,
            shape,
            angle,
            height_sw,
            height_se,
            height_ne,
            height_nw,
            anim: Some(seq),
            anim_frame,
            anim_cycle,
            min_y: 1000,
        }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, loop_cycle: i32) -> Option<Model> {
        if let Some(seq_id) = self.anim {
            let seq = cache.seq(seq_id);
            let mut delta = loop_cycle - self.anim_cycle;
            if delta > 100 && seq.loops > 0 {
                delta = 100;
            }

            while delta > seq.get_delay(self.anim_frame) {
                delta -= seq.get_delay(self.anim_frame);
                self.anim_frame += 1;

                if self.anim_frame < seq.num_frames {
                    continue;
                }

                self.anim_frame -= seq.loops;

                if self.anim_frame < 0 || self.anim_frame >= seq.num_frames {
                    self.anim = None;
                    break;
                }
            }

            self.anim_cycle = loop_cycle - delta;
        }

        let mut frame = -1;
        if let Some(seq_id) = self.anim {
            if let Some(frames) = &cache.seq(seq_id).frames {
                if let Some(&f) = frames.get(self.anim_frame as usize) {
                    frame = f;
                }
            }
        }

        let mut model = cache.loc(self.index as usize).get_model(
            cache,
            self.shape,
            self.angle,
            self.height_sw,
            self.height_se,
            self.height_ne,
            self.height_nw,
            frame,
        )?;

        // `World.shareLight` only lights `SceneModel::Model` (its
        // `pointNormal` gate); an animated sharelight loc's frame model
        // would otherwise keep the zeroed pre-light `face_colour_a` and
        // render black (the 274 data has two such locs: 1779 "Sails" and
        // 1812 "Portal"). Light every materialised frame with the same
        // `shareLight(64, 768, -50, -10, -50)` constants the pass applies
        // to the static walls; the `point_normal` gate makes this a no-op
        // for the frames `calculate_normals` already lit inline.
        if model.point_normal.is_some() {
            let light_magnitude = ((50 * 50 + 10 * 10 + 50 * 50) as f64).sqrt() as i32;
            let attenuation = (768 * light_magnitude) >> 8;
            model.light(64, attenuation, -50, -10, -50);
        }
        Some(model)
    }
}

/// Stand-in for `Math.random()` (the TS uses it for the random start frame).
fn random_float() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((nanos >> 20) % 1_000_000) as f64 / 1_000_000.0
}
