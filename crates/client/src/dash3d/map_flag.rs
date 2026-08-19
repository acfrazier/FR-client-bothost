// Port of `~/experiments/Server/webclient/src/dash3d/MapFlag.ts`.
pub struct MapFlag;

impl MapFlag {
    pub const BLOCK: i32 = 0x1;
    pub const LINK_BELOW: i32 = 0x2;
    pub const REMOVE_ROOF: i32 = 0x4;
    pub const VIS_BELOW: i32 = 0x8;
    pub const FORCE_HIGH_DETAIL: i32 = 0x10;
}
