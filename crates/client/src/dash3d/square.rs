// Port of `~/experiments/Server/webclient/src/dash3d/Square.ts`. The TS
// `Linkable` base is dropped (it only feeds the render fill queue). Sprite
// slots are indices into the `World` sprite arena: the TS shares one sprite
// object across every tile it spans, which an arena reproduces with plain
// indices (`Send`, no `Rc`).
use crate::dash3d::{Decor, Ground, GroundDecor, GroundObject, QuickGround, Wall};

pub struct Square {
    pub level: i32,
    pub x: i32,
    pub z: i32,
    pub original_level: i32,
    pub sprites: [Option<usize>; 5],
    pub sprite_span: [i32; 5],
    pub quick_ground: Option<QuickGround>,
    pub ground: Option<Ground>,
    pub wall: Option<Wall>,
    pub decor: Option<Decor>,
    pub ground_decor: Option<GroundDecor>,
    pub ground_object: Option<GroundObject>,
    pub linked_square: Option<Box<Square>>,
    pub sprite_count: i32,
    pub sprite_spans: i32,
    pub draw_level: i32,
    /// Render-pass flags, read and written by `World::render_all`/`fill`
    /// (the TS fields with the same names on `Square`).
    pub draw_front: bool,
    pub draw_back: bool,
    pub draw_sprites: bool,
    pub corner_sides: i32,
    pub sides_before_corner: i32,
    pub sides_after_corner: i32,
    pub back_wall_types: i32,
    /// Stamp of this square's live `fillQueue` entry. Java/TS `LinkList.push`
    /// unlinks a node already in the list and appends it at the tail; a
    /// matching stamp makes older deque copies stale the same way.
    pub fill_stamp: i32,
    /// Bumped by every model-bearing mutation (`set_wall`/`set_decor`/
    /// `set_ground_decor`/`set_obj`/`add_scenery`/the LOC_ANIM arm). The
    /// render side (`RenderWorld`) re-resolves the tile's models when it
    /// changes (Task 3b lazy decode).
    pub model_stamp: i32,
}

impl Square {
    pub fn new(level: i32, x: i32, z: i32) -> Self {
        Square {
            level,
            x,
            z,
            original_level: level,
            sprites: [None; 5],
            sprite_span: [0; 5],
            quick_ground: None,
            ground: None,
            wall: None,
            decor: None,
            ground_decor: None,
            ground_object: None,
            linked_square: None,
            sprite_count: 0,
            sprite_spans: 0,
            draw_level: 0,
            draw_front: false,
            draw_back: false,
            draw_sprites: false,
            corner_sides: 0,
            sides_before_corner: 0,
            sides_after_corner: 0,
            back_wall_types: 0,
            fill_stamp: 0,
            model_stamp: 0,
        }
    }
}
