//! 1:1 port of `~/experiments/Server/webclient/src/dash3d/` for the scene
//! graph, collision and entities (Task 15). The 3D render pass
//! (`World.renderAll`/`fill`, `Model.objRender`/`worldRender`, Pix3D draws)
//! is deferred to the render task; everything needed to build the scene and
//! answer collision queries is here.
//!
//! The port is a faithful transcription of the TS, so several clippy styles
//! that would rewrite the structure (branch merging, tmp-swaps, argument
//! counts, scalar clamps) are allowed for the whole module tree.
#![allow(clippy::collapsible_match)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_swap)]
#![allow(clippy::question_mark)]
#![allow(clippy::too_many_arguments)]

pub mod anim_base;
pub mod anim_frame;
pub mod client_entity;
pub mod client_loc_anim;
pub mod client_npc;
pub mod client_obj;
pub mod client_player;
pub mod client_proj;
pub mod collision_flag;
pub mod collision_map;
pub mod decor;
pub mod direction_flag;
pub mod ground;
pub mod ground_decor;
pub mod ground_object;
pub mod loc_angle;
pub mod loc_change;
pub mod loc_layer;
pub mod loc_shape;
pub mod map_flag;
pub mod map_spot_anim;
pub mod model;
pub mod model_source;
pub mod occlude;
pub mod point_normal;
pub mod quick_ground;
pub mod sprite;
pub mod square;
pub mod terrain_overlay_shape;
pub mod wall;

pub use anim_base::{AnimBase, AnimTransform};
pub use anim_frame::AnimFrame;
pub use client_entity::ClientEntity;
pub use client_loc_anim::ClientLocAnim;
pub use client_npc::ClientNpc;
pub use client_obj::ClientObj;
pub use client_player::ClientPlayer;
pub use client_proj::ClientProj;
pub use collision_flag::CollisionFlag;
pub use collision_map::{BuildArea, CollisionMap};
pub use decor::Decor;
pub use direction_flag::DirectionFlag;
pub use ground::Ground;
pub use ground_decor::GroundDecor;
pub use ground_object::GroundObject;
pub use loc_angle::LocAngle;
pub use loc_change::LocChange;
pub use loc_layer::LocLayer;
pub use loc_shape::{LocShape, LOC_SHAPE_TO_LAYER};
pub use map_flag::MapFlag;
pub use map_spot_anim::MapSpotAnim;
pub use model::Model;
pub use model_source::{ModelSource, SceneModel};
pub use occlude::Occlude;
pub use point_normal::PointNormal;
pub use quick_ground::QuickGround;
pub use sprite::Sprite;
pub use square::Square;
pub use terrain_overlay_shape::TerrainOverlayShape;
pub use wall::Wall;

/// Java `int` cross product `a * b - c * d`. Screen-space facing tests
/// overflow i32 on near, large faces; matching the wrap is what paints
/// the Lumbridge fence over the hill behind it (i64 / TS doubles keep
/// the exact sign and cull those faces, leaving hill triangles).
pub(crate) fn wrapping_cross(a: i32, b: i32, c: i32, d: i32) -> i32 {
    a.wrapping_mul(b).wrapping_sub(c.wrapping_mul(d))
}

#[cfg(test)]
mod wrapping_cross_tests {
    use super::wrapping_cross;

    #[test]
    fn overflow_matches_java_int_not_i64() {
        // 50000² = 2.5e9 overflows i32 to a negative; i64 stays positive.
        assert!(50000i64 * 50000 > 0);
        assert!(wrapping_cross(50000, 50000, 0, 0) < 0);

        // 50000 * -50000 overflows i32 to a positive; i64 stays negative.
        // Java draws this face; an i64 facing test would cull it (holes).
        assert!(50000i64 * -50000 < 0);
        assert!(wrapping_cross(50000, -50000, 0, 0) > 0);
    }
}
