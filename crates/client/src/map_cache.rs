//! Pure, checked client-map format helpers shared by the live scene builder and
//! the local world-map baker. These routines decode bytes only: they never
//! construct a `ClientBuild`, `World`, model store, or renderer.

use crate::graphics::{Pix3D, Pix8};
use std::fmt;

pub const MAP_PLANES: u8 = 4;
pub const MAP_SQUARE_SIZE: u8 = 64;
pub const MAP_INDEX_ROW_BYTES: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapIndexEntry {
    pub square_x: u8,
    pub square_z: u8,
    pub land_file: u16,
    pub loc_file: u16,
    pub free_to_play: bool,
}

impl MapIndexEntry {
    pub fn packed_square(self) -> u16 {
        u16::from(self.square_x) << 8 | u16::from(self.square_z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandCell {
    pub plane: u8,
    pub x: u8,
    pub z: u8,
    /// Explicit height opcode after the client's `1 -> 0` normalization.
    /// `None` means the deterministic terrain fallback on plane zero and
    /// `previous plane - 240` on higher planes.
    pub explicit_height: Option<u8>,
    pub flags: u8,
    pub underlay: u8,
    pub overlay: u8,
    pub overlay_shape: u8,
    pub overlay_rotation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocPlacement {
    pub id: u32,
    pub plane: u8,
    pub x: u8,
    pub z: u8,
    pub shape: u8,
    pub rotation: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapCacheError {
    Truncated(&'static str),
    Invalid(&'static str),
    Duplicate(&'static str),
    Limit(&'static str),
}

impl fmt::Display for MapCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated(what) => write!(f, "truncated client map {what}"),
            Self::Invalid(what) => write!(f, "invalid client map {what}"),
            Self::Duplicate(what) => write!(f, "duplicate client map {what}"),
            Self::Limit(what) => write!(f, "client map limit exceeded: {what}"),
        }
    }
}

impl std::error::Error for MapCacheError {}

/// Decode `versionlist`'s `map_index` member. Rows are exactly
/// `(packed square:g2, land file:g2, loc file:g2, free:g1)`.
pub fn decode_map_index(bytes: &[u8]) -> Result<Vec<MapIndexEntry>, MapCacheError> {
    let (rows, remainder) = bytes.as_chunks::<MAP_INDEX_ROW_BYTES>();
    if rows.is_empty() || !remainder.is_empty() {
        return Err(MapCacheError::Invalid("index row width"));
    }
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let square = u16::from_be_bytes([row[0], row[1]]);
        let free = row[6];
        if free > 1 {
            return Err(MapCacheError::Invalid("free-world flag"));
        }
        entries.push(MapIndexEntry {
            square_x: (square >> 8) as u8,
            square_z: square as u8,
            land_file: u16::from_be_bytes([row[2], row[3]]),
            loc_file: u16::from_be_bytes([row[4], row[5]]),
            free_to_play: free != 0,
        });
    }
    entries.sort_unstable_by_key(|entry| entry.packed_square());
    if entries
        .windows(2)
        .any(|pair| pair[0].packed_square() == pair[1].packed_square())
    {
        return Err(MapCacheError::Duplicate("square"));
    }
    Ok(entries)
}

/// Allocation-free fidelity extraction of `ClientBuild::load_ground`'s opcode
/// loop. Every one of the four 64x64 cells is visited in client stream order.
pub fn visit_land(bytes: &[u8], mut visit: impl FnMut(LandCell)) -> Result<(), MapCacheError> {
    let mut input = Cursor::new(bytes);
    for plane in 0..MAP_PLANES {
        for x in 0..MAP_SQUARE_SIZE {
            for z in 0..MAP_SQUARE_SIZE {
                let mut cell = LandCell {
                    plane,
                    x,
                    z,
                    explicit_height: None,
                    flags: 0,
                    underlay: 0,
                    overlay: 0,
                    overlay_shape: 0,
                    overlay_rotation: 0,
                };
                loop {
                    let opcode = input.u8("land opcode")?;
                    match opcode {
                        0 => break,
                        1 => {
                            let height = input.u8("land height")?;
                            cell.explicit_height = Some(if height == 1 { 0 } else { height });
                            break;
                        }
                        2..=49 => {
                            // The native client reads this as g1b into a
                            // Uint8Array. Retaining its bits is equivalent.
                            cell.overlay = input.u8("land overlay")?;
                            cell.overlay_shape = (opcode - 2) / 4;
                            cell.overlay_rotation = (opcode - 2) & 3;
                        }
                        50..=81 => cell.flags = opcode - 49,
                        _ => cell.underlay = opcode - 81,
                    }
                }
                visit(cell);
            }
        }
    }
    if input.remaining() != 0 {
        return Err(MapCacheError::Invalid("trailing land bytes"));
    }
    Ok(())
}

/// Allocation-free fidelity extraction of `ClientBuild::load_locations`'s
/// delta/smart loop. The callback is invoked once per static LOC placement;
/// NPC coordinates are not present in this format.
pub fn visit_locations(
    bytes: &[u8],
    mut visit: impl FnMut(LocPlacement),
) -> Result<(), MapCacheError> {
    let mut input = Cursor::new(bytes);
    let mut loc_id = -1i64;
    loop {
        let delta_id = i64::from(input.smart("location id delta")?);
        if delta_id == 0 {
            return if input.remaining() == 0 {
                Ok(())
            } else {
                Err(MapCacheError::Invalid("trailing location bytes"))
            };
        }
        loc_id = loc_id
            .checked_add(delta_id)
            .ok_or(MapCacheError::Limit("location id"))?;
        let id = u32::try_from(loc_id).map_err(|_| MapCacheError::Invalid("location id"))?;
        let mut position = 0u32;
        loop {
            let delta_position = u32::from(input.smart("location position delta")?);
            if delta_position == 0 {
                break;
            }
            position = position
                .checked_add(delta_position - 1)
                .ok_or(MapCacheError::Limit("location position"))?;
            let plane = position >> 12;
            if plane >= u32::from(MAP_PLANES) || position >= 1 << 14 {
                return Err(MapCacheError::Invalid("location position"));
            }
            let info = input.u8("location shape")?;
            let shape = info >> 2;
            if shape > 22 {
                return Err(MapCacheError::Invalid("location shape"));
            }
            visit(LocPlacement {
                id,
                plane: plane as u8,
                x: ((position >> 6) & 0x3f) as u8,
                z: (position & 0x3f) as u8,
                shape,
                rotation: info & 3,
            });
        }
    }
}

/// Height used by plane-zero opcode-0 land cells. This is the native client's
/// perlin fallback with the `load_ground` world-coordinate offsets included.
pub fn terrain_height(world_x: i32, world_z: i32) -> i32 {
    -perlin_noise(world_x + 932_731, world_z + 556_238) * 8
}

/// Native `Pix3D::get_texture_average` without constructing `Pix3D` or its
/// texture/frame pools. The map baker can depack one `Pix8` at a time.
pub fn texture_average(texture: &Pix8) -> u32 {
    if texture.bpal.is_empty() {
        return 1;
    }
    let mut red = 0u64;
    let mut green = 0u64;
    let mut blue = 0u64;
    for &rgb in &texture.bpal {
        let corrected = gamma_correct(rgb, 0.8);
        red += ((corrected >> 16) & 0xff) as u64;
        green += ((corrected >> 8) & 0xff) as u64;
        blue += (corrected & 0xff) as u64;
    }
    let count = texture.bpal.len() as u64;
    let average =
        (((red / count) as i32) << 16) | (((green / count) as i32) << 8) | (blue / count) as i32;
    gamma_correct(average, 1.4).max(1) as u32
}

fn gamma_correct(rgb: i32, gamma: f64) -> i32 {
    let red = ((rgb >> 16) as f64 / 256.0).powf(gamma);
    let green = (((rgb >> 8) & 0xff) as f64 / 256.0).powf(gamma);
    let blue = ((rgb & 0xff) as f64 / 256.0).powf(gamma);
    (((red * 256.0) as i32) << 16) | (((green * 256.0) as i32) << 8) | (blue * 256.0) as i32
}

fn perlin_noise(x: i32, z: i32) -> i32 {
    let value = interpolated_noise(x + 45_365, z + 91_923, 4)
        + ((interpolated_noise(x + 10_294, z + 37_821, 2) - 128) >> 1)
        + ((interpolated_noise(x, z, 1) - 128) >> 2)
        - 128;
    (((value as f64 * 0.3) as i32) + 35).clamp(10, 60)
}

fn interpolated_noise(x: i32, z: i32, scale: i32) -> i32 {
    let int_x = x / scale;
    let frac_x = x & (scale - 1);
    let int_z = z / scale;
    let frac_z = z & (scale - 1);
    let v1 = smooth_noise(int_x, int_z);
    let v2 = smooth_noise(int_x + 1, int_z);
    let v3 = smooth_noise(int_x, int_z + 1);
    let v4 = smooth_noise(int_x + 1, int_z + 1);
    let i1 = interpolate(v1, v2, frac_x, scale);
    let i2 = interpolate(v3, v4, frac_x, scale);
    interpolate(i1, i2, frac_z, scale)
}

fn interpolate(a: i32, b: i32, x: i32, scale: i32) -> i32 {
    let factor = (65_536 - Pix3D::cos_table()[((x * 1024) / scale) as usize]) >> 1;
    ((a * (65_536 - factor)) >> 16) + ((b * factor) >> 16)
}

fn smooth_noise(x: i32, y: i32) -> i32 {
    let corners =
        noise(x - 1, y - 1) + noise(x + 1, y - 1) + noise(x - 1, y + 1) + noise(x + 1, y + 1);
    let sides = noise(x - 1, y) + noise(x + 1, y) + noise(x, y - 1) + noise(x, y + 1);
    (corners / 16) + (sides / 8) + (noise(x, y) / 4)
}

fn noise(x: i32, y: i32) -> i32 {
    let n = x.wrapping_add(y.wrapping_mul(57));
    let n1 = ((n << 13) ^ n) as i128;
    let value = (n1 * (n1 * n1 * 15_731 + 789_221) + 1_376_312_589) & 0x7fff_ffff;
    ((value >> 19) & 0xff) as i32
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, MapCacheError> {
        let value = *self
            .bytes
            .get(self.position)
            .ok_or(MapCacheError::Truncated(what))?;
        self.position += 1;
        Ok(value)
    }

    fn smart(&mut self, what: &'static str) -> Result<u16, MapCacheError> {
        let first = *self
            .bytes
            .get(self.position)
            .ok_or(MapCacheError::Truncated(what))?;
        if first < 0x80 {
            self.position += 1;
            Ok(u16::from(first))
        } else {
            let second = *self
                .bytes
                .get(self.position + 1)
                .ok_or(MapCacheError::Truncated(what))?;
            self.position += 2;
            Ok(u16::from_be_bytes([first, second]) - 0x8000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_index_is_exactly_seven_byte_rows_and_canonical() {
        let rows = [
            2, 1, 0, 9, 0, 10, 1, // square 2,1
            1, 2, 0, 7, 0, 8, 0, // square 1,2
        ];
        let entries = decode_map_index(&rows).unwrap();
        assert_eq!(entries[0].packed_square(), 0x0102);
        assert_eq!(entries[1].packed_square(), 0x0201);
        assert!(entries[1].free_to_play);
        assert!(decode_map_index(&rows[..13]).is_err());
        let mut duplicate = rows.to_vec();
        duplicate.extend_from_slice(&rows[..7]);
        assert!(matches!(
            decode_map_index(&duplicate),
            Err(MapCacheError::Duplicate("square"))
        ));
    }

    #[test]
    fn land_opcodes_preserve_overlay_flags_underlay_and_height() {
        let mut bytes = vec![0; MAP_PLANES as usize * 64 * 64];
        bytes[0] = 6; // shape 1, rotation 0
        bytes.insert(1, 0xfe);
        bytes.insert(2, 51); // flags 2
        bytes.insert(3, 86); // underlay 5
        bytes.insert(4, 1);
        bytes.insert(5, 1); // normalized explicit zero
        let mut first = None;
        visit_land(&bytes, |cell| {
            if cell.plane == 0 && cell.x == 0 && cell.z == 0 {
                first = Some(cell);
            }
        })
        .unwrap();
        let first = first.unwrap();
        assert_eq!(first.overlay, 0xfe);
        assert_eq!(first.overlay_shape, 1);
        assert_eq!(first.flags, 2);
        assert_eq!(first.underlay, 5);
        assert_eq!(first.explicit_height, Some(0));
    }

    #[test]
    fn location_stream_visits_deltas_without_collecting() {
        // id -1 + 2 = 1; position 0 + (65 - 1) = 64 => x=1,z=0;
        // then + (4097 - 1) => plane=1,x=1,z=0.
        let bytes = [2, 65, 42, 0x90, 0x01, 3, 0, 0];
        let mut placements = Vec::new();
        visit_locations(&bytes, |placement| placements.push(placement)).unwrap();
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].id, 1);
        assert_eq!(
            (placements[0].plane, placements[0].x, placements[0].z),
            (0, 1, 0)
        );
        assert_eq!((placements[0].shape, placements[0].rotation), (10, 2));
        assert_eq!(placements[1].plane, 1);
        assert_eq!((placements[1].shape, placements[1].rotation), (0, 3));
    }

    #[test]
    fn map_streams_reject_truncation_and_trailing_data() {
        let land = vec![0; MAP_PLANES as usize * 64 * 64];
        assert!(matches!(
            visit_land(&land[..land.len() - 1], |_| {}),
            Err(MapCacheError::Truncated("land opcode"))
        ));
        let mut trailing_land = land;
        trailing_land.push(0);
        assert!(matches!(
            visit_land(&trailing_land, |_| {}),
            Err(MapCacheError::Invalid("trailing land bytes"))
        ));

        assert!(matches!(
            visit_locations(&[1], |_| {}),
            Err(MapCacheError::Truncated("location position delta"))
        ));
        assert!(matches!(
            visit_locations(&[0, 0], |_| {}),
            Err(MapCacheError::Invalid("trailing location bytes"))
        ));
    }
}
