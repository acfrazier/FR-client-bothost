// Port of `~/experiments/Server/webclient/src/dash3d/DirectionFlag.ts`.
pub struct DirectionFlag;

impl DirectionFlag {
    pub const NORTH: i32 = 0x1;
    pub const EAST: i32 = 0x2;
    pub const SOUTH: i32 = 0x4;
    pub const WEST: i32 = 0x8;
}
