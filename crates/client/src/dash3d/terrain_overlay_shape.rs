// Port of `~/experiments/Server/webclient/src/dash3d/TerrainOverlayShape.ts`.
pub struct TerrainOverlayShape;

impl TerrainOverlayShape {
    pub const PLAIN: i32 = 0;
    pub const DIAGONAL: i32 = 1;
    pub const LEFT_SEMI_DIAGONAL_SMALL: i32 = 2;
    pub const RIGHT_SEMI_DIAGONAL_SMALL: i32 = 3;
    pub const LEFT_SEMI_DIAGONAL_BIG: i32 = 4;
    pub const RIGHT_SEMI_DIAGONAL_BIG: i32 = 5;
    pub const HALF_SQUARE: i32 = 6;
    pub const CORNER_SMALL: i32 = 7;
    pub const CORNER_BIG: i32 = 8;
    pub const FAN_SMALL: i32 = 9;
    pub const FAN_BIG: i32 = 10;
    pub const TRAPEZIUM: i32 = 11;
}
