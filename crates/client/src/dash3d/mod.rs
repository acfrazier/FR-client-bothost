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
pub mod collision_flag;
pub mod collision_map;
pub mod decor;
pub mod direction_flag;
pub mod ground;
pub mod ground_decor;
pub mod ground_object;
pub mod loc_angle;
pub mod loc_layer;
pub mod loc_shape;
pub mod map_flag;
pub mod model;
pub mod model_source;
pub mod occlude;
pub mod point_normal;
pub mod quick_ground;
pub mod sprite;
pub mod square;
pub mod terrain_overlay_shape;
pub mod wall;
pub mod world;

pub use anim_base::{AnimBase, AnimTransform};
pub use anim_frame::AnimFrame;
pub use client_entity::ClientEntity;
pub use client_loc_anim::ClientLocAnim;
pub use client_npc::ClientNpc;
pub use client_obj::ClientObj;
pub use client_player::ClientPlayer;
pub use collision_flag::CollisionFlag;
pub use collision_map::{BuildArea, CollisionMap};
pub use decor::Decor;
pub use direction_flag::DirectionFlag;
pub use ground::Ground;
pub use ground_decor::GroundDecor;
pub use ground_object::GroundObject;
pub use loc_angle::LocAngle;
pub use loc_layer::LocLayer;
pub use loc_shape::{LocShape, LOC_SHAPE_TO_LAYER};
pub use map_flag::MapFlag;
pub use model::Model;
pub use model_source::{ModelSource, SceneModel};
pub use occlude::Occlude;
pub use point_normal::PointNormal;
pub use quick_ground::QuickGround;
pub use sprite::Sprite;
pub use square::Square;
pub use terrain_overlay_shape::TerrainOverlayShape;
pub use wall::Wall;
pub use world::World;
