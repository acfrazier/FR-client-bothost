// Port of `~/experiments/Server/webclient/src/dash3d/LocShape.ts`.
use crate::dash3d::loc_layer::LocLayer;

pub struct LocShape;

impl LocShape {
    pub const WALL_STRAIGHT: i32 = 0;
    pub const WALL_DIAGONAL_CORNER: i32 = 1;
    pub const WALL_L: i32 = 2;
    pub const WALL_SQUARE_CORNER: i32 = 3;

    pub const WALLDECOR_STRAIGHT_NOOFFSET: i32 = 4;
    pub const WALLDECOR_STRAIGHT_OFFSET: i32 = 5;
    pub const WALLDECOR_DIAGONAL_OFFSET: i32 = 6;
    pub const WALLDECOR_DIAGONAL_NOOFFSET: i32 = 7;
    pub const WALLDECOR_DIAGONAL_BOTH: i32 = 8;

    pub const WALL_DIAGONAL: i32 = 9;
    pub const CENTREPIECE_STRAIGHT: i32 = 10;
    pub const CENTREPIECE_DIAGONAL: i32 = 11;
    pub const ROOF_STRAIGHT: i32 = 12;
    pub const ROOF_DIAGONAL_WITH_ROOFEDGE: i32 = 13;
    pub const ROOF_DIAGONAL: i32 = 14;
    pub const ROOF_L_CONCAVE: i32 = 15;
    pub const ROOF_L_CONVEX: i32 = 16;
    pub const ROOF_FLAT: i32 = 17;
    pub const ROOFEDGE_STRAIGHT: i32 = 18;
    pub const ROOFEDGE_DIAGONAL_CORNER: i32 = 19;
    pub const ROOFEDGE_L: i32 = 20;
    pub const ROOFEDGE_SQUARE_CORNER: i32 = 21;

    pub const GROUND_DECOR: i32 = 22;
}

pub const LOC_SHAPE_TO_LAYER: [i32; 23] = [
    LocLayer::WALL,
    LocLayer::WALL,
    LocLayer::WALL,
    LocLayer::WALL,
    LocLayer::WALL_DECOR,
    LocLayer::WALL_DECOR,
    LocLayer::WALL_DECOR,
    LocLayer::WALL_DECOR,
    LocLayer::WALL_DECOR,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND,
    LocLayer::GROUND_DECOR,
];
