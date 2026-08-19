//! Port of `~/experiments/Server/webclient/src/client/ClientBuild.ts` — the
//! map-build scratch arrays and the ground decode (`loadGround`, plus the
//! perlin-noise terrain fallback it calls). The scene build
//! (`loadLocations`/`addLoc`, `finishBuild` with the light/occlusion passes)
//! lands with the map-completion flow (needs OnDemand map data and the render
//! pass); only the ground decode is ported here.
//!
//! `loadGround` writes the client's `groundh` and `mapl` directly (the TS
//! passes its `Client.groundh`/`Client.mapl` references into the
//! constructor); the per-build floor arrays live on this struct as in TS.
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::BuildArea;
use crate::graphics::Pix3D;
use crate::io::Packet;

pub struct ClientBuild {
    /// `floort1[level][x][z]` / `floort2[level][x][z]` floor type ids.
    floort1: Vec<Vec<Vec<u8>>>,
    floort2: Vec<Vec<Vec<u8>>>,
    /// `floors[level][x][z]` overlay shape, `floorr[level][x][z]` rotation.
    floors: Vec<Vec<Vec<u8>>>,
    floorr: Vec<Vec<Vec<u8>>>,
}

impl Default for ClientBuild {
    fn default() -> Self {
        ClientBuild::new()
    }
}

impl ClientBuild {
    /// One build-area (`BuildArea.SIZE` tiles square) of scratch arrays, as
    /// `new ClientBuild(BuildArea.SIZE, BuildArea.SIZE, ...)` in client-ts.
    pub fn new() -> Self {
        let grid = || {
            vec![
                vec![vec![0u8; BuildArea::SIZE as usize]; BuildArea::SIZE as usize];
                BuildArea::LEVELS as usize
            ]
        };
        ClientBuild {
            floort1: grid(),
            floort2: grid(),
            floors: grid(),
            floorr: grid(),
        }
    }

    /// `loadGround(src, originX, originZ, xOffset, zOffset)` from client-ts:
    /// decode one 64x64x4 map square into `groundh` and `mapl`. `origin` is
    /// the build base (`(centreZone - 6) * 8`); `xOffset`/`zOffset` are the
    /// square's local tiles. Out-of-area tiles still consume the packet
    /// bytes.
    pub fn load_ground(
        &mut self,
        groundh: &mut LevelHeightmaps,
        mapl: &mut Vec<Vec<Vec<u8>>>,
        src: &[u8],
        origin_x: i32,
        origin_z: i32,
        x_offset: i32,
        z_offset: i32,
    ) {
        let mut buf = Packet::new(src.to_vec());

        for level in 0..BuildArea::LEVELS {
            for x in 0..64 {
                for z in 0..64 {
                    let stx = x + x_offset;
                    let stz = z + z_offset;

                    if (0..BuildArea::SIZE).contains(&stx) && (0..BuildArea::SIZE).contains(&stz) {
                        mapl[level as usize][stx as usize][stz as usize] = 0;

                        loop {
                            let opcode = buf.g1();
                            if opcode == 0 {
                                if level == 0 {
                                    groundh[0][stx as usize][stz as usize] = -Self::perlin_noise(
                                        stx + origin_x + 932731,
                                        stz + 556238 + origin_z,
                                    ) * 8;
                                } else {
                                    groundh[level as usize][stx as usize][stz as usize] =
                                        groundh[level as usize - 1][stx as usize][stz as usize] - 240;
                                }
                                break;
                            }

                            if opcode == 1 {
                                let mut height = buf.g1();
                                if height == 1 {
                                    height = 0;
                                }
                                if level == 0 {
                                    groundh[0][stx as usize][stz as usize] = -height * 8;
                                } else {
                                    groundh[level as usize][stx as usize][stz as usize] =
                                        groundh[level as usize - 1][stx as usize][stz as usize]
                                            - height * 8;
                                }
                                break;
                            }

                            if opcode <= 49 {
                                // g1b into a Uint8Array: signed byte, stored raw
                                self.floort2[level as usize][stx as usize][stz as usize] =
                                    buf.g1b() as u8;
                                self.floors[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 2) / 4) << 24 >> 24) as u8;
                                self.floorr[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 2) & 0x3) << 24 >> 24) as u8;
                            } else if opcode <= 81 {
                                mapl[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 49) << 24) >> 24) as u8;
                            } else {
                                self.floort1[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 81) << 24) >> 24) as u8;
                            }
                        }
                    } else {
                        loop {
                            let opcode = buf.g1();
                            if opcode == 0 {
                                break;
                            }

                            if opcode == 1 {
                                buf.g1();
                                break;
                            }

                            if opcode <= 49 {
                                buf.g1();
                            }
                        }
                    }
                }
            }
        }
    }

    /// `perlinNoise(x, z)` from client-ts: fallback terrain for map squares
    /// with no ground data (level 0 opcode-0 tiles).
    fn perlin_noise(x: i32, z: i32) -> i32 {
        let value = Self::interpolated_noise(x + 45365, z + 91923, 4)
            + ((Self::interpolated_noise(x + 10294, z + 37821, 2) - 128) >> 1)
            + ((Self::interpolated_noise(x, z, 1) - 128) >> 2)
            - 128;
        let value = ((value as f64 * 0.3) as i32) + 35;
        value.clamp(10, 60)
    }

    fn interpolated_noise(x: i32, z: i32, scale: i32) -> i32 {
        let int_x = x / scale;
        let frac_x = x & (scale - 1);
        let int_z = z / scale;
        let frac_z = z & (scale - 1);
        let v1 = Self::smooth_noise(int_x, int_z);
        let v2 = Self::smooth_noise(int_x + 1, int_z);
        let v3 = Self::smooth_noise(int_x, int_z + 1);
        let v4 = Self::smooth_noise(int_x + 1, int_z + 1);
        let i1 = Self::interpolate(v1, v2, frac_x, scale);
        let i2 = Self::interpolate(v3, v4, frac_x, scale);
        Self::interpolate(i1, i2, frac_z, scale)
    }

    fn interpolate(a: i32, b: i32, x: i32, scale: i32) -> i32 {
        let f = (65536 - Pix3D::cos_table()[((x * 1024) / scale) as usize]) >> 1;
        ((a * (65536 - f)) >> 16) + ((b * f) >> 16)
    }

    fn smooth_noise(x: i32, y: i32) -> i32 {
        let corners = Self::noise(x - 1, y - 1)
            + Self::noise(x + 1, y - 1)
            + Self::noise(x - 1, y + 1)
            + Self::noise(x + 1, y + 1);
        let sides =
            Self::noise(x - 1, y) + Self::noise(x + 1, y) + Self::noise(x, y - 1) + Self::noise(x, y + 1);
        let center = Self::noise(x, y);
        // i32 division truncates toward zero, matching the TS `| 0`
        corners / 16 + sides / 8 + center / 4
    }

    /// `noise(x, y)` from client-ts. The TS uses BigInt for the cubic term
    /// (int32 overflows), so this port computes it in i128 before masking.
    fn noise(x: i32, y: i32) -> i32 {
        let n = x.wrapping_add(y.wrapping_mul(57));
        let n1 = ((n << 13) ^ n) as i128;
        let v = (n1 * (n1 * n1 * 15731 + 789221) + 1376312589) & 0x7fff_ffff;
        ((v >> 19) & 0xff) as i32
    }
}
