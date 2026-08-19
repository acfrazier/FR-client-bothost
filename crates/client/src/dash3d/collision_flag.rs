// Port of `~/experiments/Server/webclient/src/dash3d/CollisionFlag.ts`.
// `WR_GRND` is the flag `CollisionMap.blockGround` sets; the `WALK_BLOCKED`
// name from the task brief does not exist in the TS source.
#![allow(dead_code)]

pub struct CollisionFlag;

impl CollisionFlag {
    pub const _OPEN: i32 = 0;

    pub const W_NW: i32 = 0x1;
    pub const W_N: i32 = 0x2;
    pub const W_NE: i32 = 0x4;
    pub const W_E: i32 = 0x8;
    pub const W_SE: i32 = 0x10;
    pub const W_S: i32 = 0x20;
    pub const W_SW: i32 = 0x40;
    pub const W_W: i32 = 0x80;
    pub const WALK_BLOCK_FLAGS: i32 = 0xFF;
    pub const WALK_SCENERY: i32 = 0x100;

    pub const V_NW: i32 = 0x200;
    pub const V_N: i32 = 0x400;
    pub const V_NE: i32 = 0x800;
    pub const V_E: i32 = 0x1000;
    pub const V_SE: i32 = 0x2000;
    pub const V_S: i32 = 0x4000;
    pub const V_SW: i32 = 0x8000;
    pub const V_W: i32 = 0x10000;
    pub const VIS_BLOCK_FLAGS: i32 = 0x1FE00;
    pub const VIS_SCENERY: i32 = 0x20000;

    pub const WR_GROUND_DECOR: i32 = 0x40000;
    pub const BLOCK_NPCS_AND_PLAYERS: i32 = 0x80000;
    pub const ROOF: i32 = 0x100000;
    pub const WR_GRND: i32 = 0x200000;

    pub const SQ_BLOCKED: i32 = 0x280100;
    pub const PL_WALK_N: i32 = 0x280102;
    pub const PL_WALK_E: i32 = 0x280108;
    pub const PL_WALK_NE: i32 = 0x28010E;
    pub const PL_WALK_S: i32 = 0x280120;
    pub const PL_WALK_SE: i32 = 0x280138;
    pub const PL_WALK_W: i32 = 0x280180;
    pub const PL_WALK_NW: i32 = 0x280183;
    pub const PL_WALK_SW: i32 = 0x2801E0;

    pub const MULTIWAY: i32 = 0x400000;
    pub const FREEMAP: i32 = 0x800000;
    pub const UNLOADED: i32 = 0x1000000;
    pub const NPCS_OR_PLAYERS: i32 = 0x2000000;

    pub const _BOUNDS: i32 = 0xFFFFFF;
}
