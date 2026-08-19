// Port of `~/experiments/Server/webclient/src/dash3d/QuickGround.ts`.
pub struct QuickGround {
    pub colour_sw: i32,
    pub colour_se: i32,
    pub colour_ne: i32,
    pub colour_nw: i32,
    pub texture: i32,
    pub minimap_rgb: i32,
    pub flat: bool,
}

impl QuickGround {
    pub fn new(
        colour_sw: i32,
        colour_se: i32,
        colour_ne: i32,
        colour_nw: i32,
        texture: i32,
        minimap_rgb: i32,
        flat: bool,
    ) -> Self {
        QuickGround {
            colour_sw,
            colour_se,
            colour_ne,
            colour_nw,
            texture,
            minimap_rgb,
            flat,
        }
    }
}
