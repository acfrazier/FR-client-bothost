// Port of `~/experiments/Server/webclient/src/dash3d/ClientObj.ts`.
use crate::config::Cache;
use crate::dash3d::Model;

pub struct ClientObj {
    pub id: i32,
    pub count: i32,
}

impl ClientObj {
    pub fn new(id: i32, count: i32) -> Self {
        ClientObj { id, count }
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, _loop_cycle: i32) -> Option<Model> {
        cache.obj(self.id as usize).get_model_lit(cache, self.count)
    }
}
