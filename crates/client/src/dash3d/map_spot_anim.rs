// Port of `~/experiments/Server/webclient/src/dash3d/MapSpotAnim.ts`.
// `Client.loopCycle` moves in as parameters; `SpotType.list[id]` is looked up
// through the `Cache`.
use crate::config::Cache;
use crate::dash3d::{AnimFrame, Model};
use crate::datastruct::linkable::{LinkableTrait, Links};

#[derive(Clone)]
pub struct MapSpotAnim {
    pub links: Links,
    pub spotanim: i32,
    pub level: i32,
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub start_cycle: i32,
    pub anim_complete: bool,
    pub anim_frame: i32,
    pub anim_cycle: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl MapSpotAnim {
    pub fn new(id: i32, level: i32, x: i32, z: i32, y: i32, cycle: i32, delay: i32) -> Self {
        MapSpotAnim {
            links: Links::new(0),
            spotanim: id,
            level,
            x,
            z,
            y,
            start_cycle: cycle + delay,
            anim_complete: false,
            anim_frame: 0,
            anim_cycle: 0,
            min_y: 1000,
        }
    }

    /// `update(delta)` from client-ts.
    pub fn update(&mut self, cache: &Cache, delta: i32) {
        let Some(seq) = cache.spot(self.spotanim as usize).seq else {
            return;
        };
        let seq_type = cache.seq(seq);

        self.anim_cycle += delta;
        while self.anim_cycle > seq_type.get_delay(self.anim_frame) {
            self.anim_cycle -= seq_type.get_delay(self.anim_frame) + 1;
            self.anim_frame += 1;

            if self.anim_frame >= seq_type.num_frames {
                self.anim_frame = 0;
                self.anim_complete = true;
            }
        }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache) -> Option<Model> {
        let spot = cache.spot(self.spotanim as usize);
        let tmp = spot.get_temp_model2(cache)?;

        let mut frame = -1;
        if let Some(seq) = spot.seq {
            if let Some(frames) = &cache.seq(seq).frames {
                frame = frames.get(self.anim_frame as usize).copied().unwrap_or(-1);
            }
        }

        let mut model =
            Model::copy_for_anim(&tmp, true, AnimFrame::animate_transparencies(frame), false);

        if !self.anim_complete {
            model.prepare_anim();
            model.animate(frame);
            model.label_faces = None;
            model.label_vertices = None;
        }

        if spot.resizeh != 128 || spot.resizev != 128 {
            model.resize(spot.resizeh, spot.resizev, spot.resizeh);
        }

        if spot.angle != 0 {
            if spot.angle == 90 {
                model.rotate90();
            } else if spot.angle == 180 {
                model.rotate90();
                model.rotate90();
            } else if spot.angle == 270 {
                model.rotate90();
                model.rotate90();
                model.rotate90();
            }
        }

        model.calculate_normals(64 + spot.ambient, 850 + spot.contrast, -30, -50, -30, true);
        Some(model)
    }
}

impl LinkableTrait for MapSpotAnim {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        MapSpotAnim::new(0, 0, 0, 0, 0, 0, 0)
    }
}
