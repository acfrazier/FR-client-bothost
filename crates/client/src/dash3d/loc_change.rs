// Port of `~/experiments/Server/webclient/src/dash3d/LocChange.ts`.
use crate::datastruct::linkable::{LinkableTrait, Links};

pub struct LocChange {
    pub links: Links,
    pub level: i32,
    pub layer: i32,
    pub x: i32,
    pub z: i32,
    pub old_type: i32,
    pub old_angle: i32,
    pub old_shape: i32,
    pub new_type: i32,
    pub new_angle: i32,
    pub new_shape: i32,
    pub start_time: i32,
    pub end_time: i32,
}

impl Default for LocChange {
    fn default() -> Self {
        LocChange {
            links: Links::new(0),
            level: 0,
            layer: 0,
            x: 0,
            z: 0,
            old_type: 0,
            old_angle: 0,
            old_shape: 0,
            new_type: 0,
            new_angle: 0,
            new_shape: 0,
            start_time: 0,
            end_time: -1,
        }
    }
}

impl LinkableTrait for LocChange {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        LocChange::default()
    }
}
