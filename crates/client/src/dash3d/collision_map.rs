// Port of `~/experiments/Server/webclient/src/dash3d/CollisionMap.ts`. The TS
// `flags` is a flat `Int32Array` indexed `x * SIZE + z`; this port stores the
// same row-major layout as `Vec<[i32; SIZE]>` so callers (and the Task 15
// test) can index `flags[x][z]` directly.
use crate::dash3d::CollisionFlag;
use crate::dash3d::DirectionFlag;
use crate::dash3d::LocAngle;
use crate::dash3d::LocShape;

// a standard build area is 4x13x13 zones, or 4x104x104 tiles
pub struct BuildArea;

impl BuildArea {
    pub const LEVELS: i32 = 4;
    pub const SIZE: i32 = 13 << 3;
}

const SIZE: i32 = BuildArea::SIZE;

pub struct CollisionMap {
    pub start_x: i32,
    pub start_z: i32,
    pub size_x: i32,
    pub size_z: i32,
    pub flags: Vec<[i32; SIZE as usize]>,
}

impl CollisionMap {
    pub fn index(x: i32, z: i32) -> usize {
        (x * SIZE + z) as usize
    }

    pub fn new() -> Self {
        let mut map = CollisionMap {
            start_x: 0,
            start_z: 0,
            size_x: SIZE,
            size_z: SIZE,
            flags: vec![[0i32; SIZE as usize]; SIZE as usize],
        };
        map.reset();
        map
    }

    pub fn reset(&mut self) {
        for x in 0..self.size_x {
            for z in 0..self.size_z {
                if x == 0 || z == 0 || x == self.size_x - 1 || z == self.size_z - 1 {
                    self.flags[x as usize][z as usize] = CollisionFlag::_BOUNDS;
                } else {
                    self.flags[x as usize][z as usize] = CollisionFlag::_OPEN;
                }
            }
        }
    }

    pub fn block_ground(&mut self, tile_x: i32, tile_z: i32) {
        let x = (tile_x - self.start_x) as usize;
        let z = (tile_z - self.start_z) as usize;
        self.flags[x][z] |= CollisionFlag::WR_GRND;
    }

    pub fn unblock_ground(&mut self, tile_x: i32, tile_z: i32) {
        let x = (tile_x - self.start_x) as usize;
        let z = (tile_z - self.start_z) as usize;
        self.flags[x][z] &= !CollisionFlag::WR_GRND;
    }

    pub fn add_loc(
        &mut self,
        tile_x: i32,
        tile_z: i32,
        mut size_x: i32,
        mut size_z: i32,
        angle: i32,
        blockrange: bool,
    ) {
        let mut flags = CollisionFlag::WALK_SCENERY;
        if blockrange {
            flags |= CollisionFlag::VIS_SCENERY;
        }

        let x = tile_x - self.start_x;
        let z = tile_z - self.start_z;

        if angle == LocAngle::NORTH || angle == LocAngle::SOUTH {
            let tmp = size_x;
            size_x = size_z;
            size_z = tmp;
        }

        for tx in x..x + size_x {
            if !(tx >= 0 && tx < self.size_x) {
                continue;
            }
            for tz in z..z + size_z {
                if !(tz >= 0 && tz < self.size_z) {
                    continue;
                }
                self.add_cmap(tx, tz, flags);
            }
        }
    }

    pub fn del_loc(
        &mut self,
        tile_x: i32,
        tile_z: i32,
        mut size_x: i32,
        mut size_z: i32,
        angle: i32,
        blockrange: bool,
    ) {
        let mut flags = CollisionFlag::WALK_SCENERY;
        if blockrange {
            flags |= CollisionFlag::VIS_SCENERY;
        }

        let x = tile_x - self.start_x;
        let z = tile_z - self.start_z;

        if angle == LocAngle::NORTH || angle == LocAngle::SOUTH {
            let tmp = size_x;
            size_x = size_z;
            size_z = tmp;
        }

        for tx in x..x + size_x {
            if !(tx >= 0 && tx < self.size_x) {
                continue;
            }
            for tz in z..z + size_z {
                if !(tz >= 0 && tz < self.size_z) {
                    continue;
                }
                self.rem_cmap(tx, tz, flags);
            }
        }
    }

    pub fn add_wall(&mut self, tile_x: i32, tile_z: i32, shape: i32, angle: i32, blockrange: bool) {
        let x = tile_x - self.start_x;
        let z = tile_z - self.start_z;

        let west = if blockrange {
            CollisionFlag::V_W
        } else {
            CollisionFlag::W_W
        };
        let east = if blockrange {
            CollisionFlag::V_E
        } else {
            CollisionFlag::W_E
        };
        let north = if blockrange {
            CollisionFlag::V_N
        } else {
            CollisionFlag::W_N
        };
        let south = if blockrange {
            CollisionFlag::V_S
        } else {
            CollisionFlag::W_S
        };
        let north_west = if blockrange {
            CollisionFlag::V_NW
        } else {
            CollisionFlag::W_NW
        };
        let south_east = if blockrange {
            CollisionFlag::V_SE
        } else {
            CollisionFlag::W_SE
        };
        let north_east = if blockrange {
            CollisionFlag::V_NE
        } else {
            CollisionFlag::W_NE
        };
        let south_west = if blockrange {
            CollisionFlag::V_SW
        } else {
            CollisionFlag::W_SW
        };

        if shape == LocShape::WALL_STRAIGHT {
            if angle == LocAngle::WEST {
                self.add_cmap(x, z, west);
                self.add_cmap(x - 1, z, east);
            } else if angle == LocAngle::NORTH {
                self.add_cmap(x, z, north);
                self.add_cmap(x, z + 1, south);
            } else if angle == LocAngle::EAST {
                self.add_cmap(x, z, east);
                self.add_cmap(x + 1, z, west);
            } else if angle == LocAngle::SOUTH {
                self.add_cmap(x, z, south);
                self.add_cmap(x, z - 1, north);
            }
        } else if shape == LocShape::WALL_DIAGONAL_CORNER || shape == LocShape::WALL_SQUARE_CORNER {
            if angle == LocAngle::WEST {
                self.add_cmap(x, z, north_west);
                self.add_cmap(x - 1, z + 1, south_east);
            } else if angle == LocAngle::NORTH {
                self.add_cmap(x, z, north_east);
                self.add_cmap(x + 1, z + 1, south_west);
            } else if angle == LocAngle::EAST {
                self.add_cmap(x, z, south_east);
                self.add_cmap(x + 1, z - 1, north_west);
            } else if angle == LocAngle::SOUTH {
                self.add_cmap(x, z, south_west);
                self.add_cmap(x - 1, z - 1, north_east);
            }
        } else if shape == LocShape::WALL_L {
            if angle == LocAngle::WEST {
                self.add_cmap(x, z, north | west);
                self.add_cmap(x - 1, z, east);
                self.add_cmap(x, z + 1, south);
            } else if angle == LocAngle::NORTH {
                self.add_cmap(x, z, north | east);
                self.add_cmap(x, z + 1, south);
                self.add_cmap(x + 1, z, west);
            } else if angle == LocAngle::EAST {
                self.add_cmap(x, z, south | east);
                self.add_cmap(x + 1, z, west);
                self.add_cmap(x, z - 1, north);
            } else if angle == LocAngle::SOUTH {
                self.add_cmap(x, z, south | west);
                self.add_cmap(x, z - 1, north);
                self.add_cmap(x - 1, z, east);
            }
        }
        if blockrange {
            self.add_wall(tile_x, tile_z, shape, angle, false);
        }
    }

    pub fn del_wall(&mut self, tile_x: i32, tile_z: i32, shape: i32, angle: i32, blockrange: bool) {
        let x = tile_x - self.start_x;
        let z = tile_z - self.start_z;

        let west = if blockrange {
            CollisionFlag::V_W
        } else {
            CollisionFlag::W_W
        };
        let east = if blockrange {
            CollisionFlag::V_E
        } else {
            CollisionFlag::W_E
        };
        let north = if blockrange {
            CollisionFlag::V_N
        } else {
            CollisionFlag::W_N
        };
        let south = if blockrange {
            CollisionFlag::V_S
        } else {
            CollisionFlag::W_S
        };
        let north_west = if blockrange {
            CollisionFlag::V_NW
        } else {
            CollisionFlag::W_NW
        };
        let south_east = if blockrange {
            CollisionFlag::V_SE
        } else {
            CollisionFlag::W_SE
        };
        let north_east = if blockrange {
            CollisionFlag::V_NE
        } else {
            CollisionFlag::W_NE
        };
        let south_west = if blockrange {
            CollisionFlag::V_SW
        } else {
            CollisionFlag::W_SW
        };

        if shape == LocShape::WALL_STRAIGHT {
            if angle == LocAngle::WEST {
                self.rem_cmap(x, z, west);
                self.rem_cmap(x - 1, z, east);
            } else if angle == LocAngle::NORTH {
                self.rem_cmap(x, z, north);
                self.rem_cmap(x, z + 1, south);
            } else if angle == LocAngle::EAST {
                self.rem_cmap(x, z, east);
                self.rem_cmap(x + 1, z, west);
            } else if angle == LocAngle::SOUTH {
                self.rem_cmap(x, z, south);
                self.rem_cmap(x, z - 1, north);
            }
        } else if shape == LocShape::WALL_DIAGONAL_CORNER || shape == LocShape::WALL_SQUARE_CORNER {
            if angle == LocAngle::WEST {
                self.rem_cmap(x, z, north_west);
                self.rem_cmap(x - 1, z + 1, south_east);
            } else if angle == LocAngle::NORTH {
                self.rem_cmap(x, z, north_east);
                self.rem_cmap(x + 1, z + 1, south_west);
            } else if angle == LocAngle::EAST {
                self.rem_cmap(x, z, south_east);
                self.rem_cmap(x + 1, z - 1, north_west);
            } else if angle == LocAngle::SOUTH {
                self.rem_cmap(x, z, south_west);
                self.rem_cmap(x - 1, z - 1, north_east);
            }
        } else if shape == LocShape::WALL_L {
            if angle == LocAngle::WEST {
                self.rem_cmap(x, z, north | west);
                self.rem_cmap(x - 1, z, east);
                self.rem_cmap(x, z + 1, south);
            } else if angle == LocAngle::NORTH {
                self.rem_cmap(x, z, north | east);
                self.rem_cmap(x, z + 1, south);
                self.rem_cmap(x + 1, z, west);
            } else if angle == LocAngle::EAST {
                self.rem_cmap(x, z, south | east);
                self.rem_cmap(x + 1, z, west);
                self.rem_cmap(x, z - 1, north);
            } else if angle == LocAngle::SOUTH {
                self.rem_cmap(x, z, south | west);
                self.rem_cmap(x, z - 1, north);
                self.rem_cmap(x - 1, z, east);
            }
        }
        if blockrange {
            self.del_wall(tile_x, tile_z, shape, angle, false);
        }
    }

    pub fn test_wall(
        &self,
        src_x: i32,
        src_z: i32,
        dst_x: i32,
        dst_z: i32,
        shape: i32,
        angle: i32,
    ) -> bool {
        if src_x == dst_x && src_z == dst_z {
            return true;
        }

        let sx = src_x - self.start_x;
        let sz = src_z - self.start_z;
        let dx = dst_x - self.start_x;
        let dz = dst_z - self.start_z;

        if shape == LocShape::WALL_STRAIGHT {
            if angle == LocAngle::WEST {
                if sx == dx - 1 && sz == dz {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::NORTH {
                if sx == dx && sz == dz + 1 {
                    return true;
                } else if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::EAST {
                if sx == dx + 1 && sz == dz {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::SOUTH {
                if sx == dx && sz == dz - 1 {
                    return true;
                } else if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            }
        } else if shape == LocShape::WALL_L {
            if angle == LocAngle::WEST {
                if sx == dx - 1 && sz == dz {
                    return true;
                } else if sx == dx && sz == dz + 1 {
                    return true;
                } else if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::NORTH {
                if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx && sz == dz + 1 {
                    return true;
                } else if sx == dx + 1 && sz == dz {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::EAST {
                if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx + 1 && sz == dz {
                    return true;
                } else if sx == dx && sz == dz - 1 {
                    return true;
                }
            } else if angle == LocAngle::SOUTH {
                if sx == dx - 1 && sz == dz {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx && sz == dz - 1 {
                    return true;
                }
            }
        } else if shape == LocShape::WALL_DIAGONAL {
            if sx == dx
                && sz == dz + 1
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_S)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx
                && sz == dz - 1
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_N)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx - 1
                && sz == dz
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_E)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx + 1
                && sz == dz
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_W)
                    == CollisionFlag::_OPEN
            {
                return true;
            }
        }
        false
    }

    pub fn test_w_decor(
        &self,
        src_x: i32,
        src_z: i32,
        dst_x: i32,
        dst_z: i32,
        shape: i32,
        mut angle: i32,
    ) -> bool {
        if src_x == dst_x && src_z == dst_z {
            return true;
        }

        let sx = src_x - self.start_x;
        let sz = src_z - self.start_z;
        let dx = dst_x - self.start_x;
        let dz = dst_z - self.start_z;

        if shape == LocShape::WALLDECOR_DIAGONAL_OFFSET
            || shape == LocShape::WALLDECOR_DIAGONAL_NOOFFSET
        {
            if shape == LocShape::WALLDECOR_DIAGONAL_NOOFFSET {
                angle = (angle + 2) & 0x3;
            }

            if angle == LocAngle::WEST {
                if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::NORTH {
                if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz - 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_N)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::EAST {
                if sx == dx - 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_E)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            } else if angle == LocAngle::SOUTH {
                if sx == dx + 1
                    && sz == dz
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_W)
                        == CollisionFlag::_OPEN
                {
                    return true;
                } else if sx == dx
                    && sz == dz + 1
                    && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_S)
                        == CollisionFlag::_OPEN
                {
                    return true;
                }
            }
        } else if shape == LocShape::WALLDECOR_DIAGONAL_BOTH {
            if sx == dx
                && sz == dz + 1
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_S)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx
                && sz == dz - 1
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_N)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx - 1
                && sz == dz
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_E)
                    == CollisionFlag::_OPEN
            {
                return true;
            } else if sx == dx + 1
                && sz == dz
                && (self.flags[sx as usize][sz as usize] & CollisionFlag::W_W)
                    == CollisionFlag::_OPEN
            {
                return true;
            }
        }
        false
    }

    pub fn test_loc(
        &self,
        src_x: i32,
        src_z: i32,
        dst_x: i32,
        dst_z: i32,
        dst_size_x: i32,
        dst_size_z: i32,
        forceapproach: i32,
    ) -> bool {
        let max_x = dst_x + dst_size_x - 1;
        let max_z = dst_z + dst_size_z - 1;
        let flag = self.flags[(src_x - self.start_x) as usize][(src_z - self.start_z) as usize];

        if src_x >= dst_x && src_x <= max_x && src_z >= dst_z && src_z <= max_z {
            return true;
        } else if src_x == dst_x - 1
            && src_z >= dst_z
            && src_z <= max_z
            && flag & CollisionFlag::W_E == CollisionFlag::_OPEN
            && forceapproach & DirectionFlag::WEST == CollisionFlag::_OPEN
        {
            return true;
        } else if src_x == max_x + 1
            && src_z >= dst_z
            && src_z <= max_z
            && flag & CollisionFlag::W_W == CollisionFlag::_OPEN
            && forceapproach & DirectionFlag::EAST == CollisionFlag::_OPEN
        {
            return true;
        } else if src_z == dst_z - 1
            && src_x >= dst_x
            && src_x <= max_x
            && flag & CollisionFlag::W_N == CollisionFlag::_OPEN
            && forceapproach & DirectionFlag::SOUTH == CollisionFlag::_OPEN
        {
            return true;
        } else if src_z == max_z + 1
            && src_x >= dst_x
            && src_x <= max_x
            && flag & CollisionFlag::W_S == CollisionFlag::_OPEN
            && forceapproach & DirectionFlag::NORTH == CollisionFlag::_OPEN
        {
            return true;
        }
        false
    }

    fn add_cmap(&mut self, x: i32, z: i32, flags: i32) {
        self.flags[x as usize][z as usize] |= flags;
    }

    fn rem_cmap(&mut self, x: i32, z: i32, flags: i32) {
        self.flags[x as usize][z as usize] &= CollisionFlag::_BOUNDS - flags;
    }
}

impl Default for CollisionMap {
    fn default() -> Self {
        Self::new()
    }
}
