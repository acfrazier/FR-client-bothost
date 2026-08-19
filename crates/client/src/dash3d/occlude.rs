// Port of `~/experiments/Server/webclient/src/dash3d/Occlude.ts`.
pub struct Occlude {
    pub min_tile_x: i32,
    pub max_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_z: i32,
    pub r#type: i32,
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub mode: i32,
    pub min_delta_x: i32,
    pub max_delta_x: i32,
    pub min_delta_z: i32,
    pub max_delta_z: i32,
    pub min_delta_y: i32,
    pub max_delta_y: i32,
}

impl Occlude {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        min_tile_x: i32,
        max_tile_x: i32,
        min_tile_z: i32,
        max_tile_z: i32,
        r#type: i32,
        min_x: i32,
        max_x: i32,
        min_z: i32,
        max_z: i32,
        min_y: i32,
        max_y: i32,
    ) -> Self {
        Occlude {
            min_tile_x,
            max_tile_x,
            min_tile_z,
            max_tile_z,
            r#type,
            min_x,
            max_x,
            min_z,
            max_z,
            min_y,
            max_y,
            mode: 0,
            min_delta_x: 0,
            max_delta_x: 0,
            min_delta_z: 0,
            max_delta_z: 0,
            min_delta_y: 0,
            max_delta_y: 0,
        }
    }
}
