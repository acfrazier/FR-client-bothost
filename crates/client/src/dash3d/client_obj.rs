// Port of `~/experiments/Server/webclient/src/dash3d/ClientObj.ts`.
use crate::config::Cache;
use crate::dash3d::Model;
use crate::datastruct::linkable::{LinkableTrait, Links};

#[derive(Clone)]
pub struct ClientObj {
    pub links: Links,
    pub id: i32,
    pub count: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl ClientObj {
    pub fn new(id: i32, count: i32) -> Self {
        ClientObj { id, count, min_y: 1000, links: Links::new(0) }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, _loop_cycle: i32) -> Option<Model> {
        cache.obj(self.id as usize).get_model_lit(cache, self.count)
    }
}

impl LinkableTrait for ClientObj {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        ClientObj { id: 0, count: 0, min_y: 1000, links: Links::new(0) }
    }
}
