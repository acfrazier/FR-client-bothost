// Port of `~/experiments/Server/webclient/src/dash3d/ClientObj.ts`.
use crate::config::Cache;
use crate::dash3d::Model;

pub struct ClientObj {
    pub id: i32,
    pub count: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl ClientObj {
    pub fn new(id: i32, count: i32) -> Self {
        ClientObj { id, count, min_y: 1000 }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, _loop_cycle: i32) -> Option<Model> {
        cache.obj(self.id as usize).get_model_lit(cache, self.count)
    }
}
