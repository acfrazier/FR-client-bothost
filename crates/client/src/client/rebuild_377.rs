//! Revision-377 region and normal scene-rebuild packet decoding.
//!
//! Java anchors: `Client.java:8087-8119` (wire decode), `8125-8135`
//! (scene base and tutorial area), and `8139-8233` (map-square ordering and
//! instance-template interpretation). This module only decodes immutable packet
//! meaning; scene mutation, map-file lookup, OnDemand requests and loading are
//! later apply effects.

use super::{PacketContext377, PacketError377, PacketErrorKind377, PacketReader377};

const REGION_PLANES: usize = 4;
const REGION_SIDE: usize = 13;
const REGION_TILE_COUNT: usize = REGION_PLANES * REGION_SIDE * REGION_SIDE;
const MAX_TEMPLATE: i32 = (1 << 26) - 1;

/// One opcode 53 scene rebuild. The instance table stays behind a box so this
/// variant does not inflate every packet's operation storage.
#[derive(Debug, PartialEq, Eq)]
pub enum Rebuild377 {
    /// Opcode 222 (`REBUILD_NORMAL`): zone coordinates after Java's transformed
    /// reads. Java reads z first with `g2`, then x with `g2_alt3`.
    Normal { zone_x: i32, zone_z: i32 },
    /// Opcode 53 (`REBUILD_REGION`): the 4 x 13 x 13 destination template grid,
    /// flattened in plane, destination-zone-x, destination-zone-z order. `-1`
    /// denotes a clear destination slot; present values retain all 26 wire bits.
    Instance {
        zone_x: i32,
        zone_z: i32,
        templates: Box<[i32]>,
    },
}

/// A present destination 8x8 zone's decoded opcode-53 source template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRegionTileTemplate377 {
    /// Destination scene plane, 0..=3.
    pub plane: usize,
    /// Absolute destination zone coordinates in the rebuilt scene.
    pub destination_zone_x: i32,
    pub destination_zone_z: i32,
    /// Absolute destination tile origin of this 8x8 zone.
    pub destination_tile_x: i32,
    pub destination_tile_z: i32,
    /// Source plane encoded in template bits 24..=25.
    pub source_level: i32,
    /// Source zone coordinates encoded in template bits 14..=23 and 3..=13.
    pub source_zone_x: i32,
    pub source_zone_z: i32,
    /// Rotation encoded in template bits 1..=2.
    pub rotation: i32,
    /// Original unsigned 26-bit template as returned by Java `gBit(26)`.
    pub packed: i32,
}

impl Rebuild377 {
    /// Decode only the two rebuild packet bodies. The packet root remains
    /// responsible for rejecting trailing bytes after this decoder returns.
    pub(super) fn decode(opcode: u8, p: &mut PacketReader377<'_>) -> Result<Self, PacketError377> {
        match opcode {
            222 => {
                // Client.java:8090-8095. Preserve Java's z-then-x read order.
                let zone_z = p.g2()?;
                let zone_x = p.g2_alt3()?;
                Ok(Self::Normal { zone_x, zone_z })
            }
            53 => {
                // Client.java:8096-8118. The final zone-z word follows bit access.
                let zone_x = p.g2_alt2()?;
                let mut bits = p.bits();
                let mut templates = Vec::with_capacity(REGION_TILE_COUNT);
                for _plane in 0..REGION_PLANES {
                    for _zone_x in 0..REGION_SIDE {
                        for _zone_z in 0..REGION_SIDE {
                            let present = bits.gbit(1)?;
                            templates.push(if present == 1 { bits.gbit(26)? } else { -1 });
                        }
                    }
                }
                bits.access_bytes();
                let zone_z = p.g2_alt2()?;
                Ok(Self::Instance {
                    zone_x,
                    zone_z,
                    templates: templates.into_boxed_slice(),
                })
            }
            _ => Err(p.error(PacketErrorKind377::UnknownOpcode)),
        }
    }

    /// Validate the decoded storage before it crosses the apply boundary.
    /// Rebuilds contain no cache/resource references, so `context` is unused.
    pub(super) fn validate(
        &self,
        _context: &impl PacketContext377,
    ) -> Result<(), PacketErrorKind377> {
        if let Self::Instance { templates, .. } = self {
            if templates.len() != REGION_TILE_COUNT {
                return Err(PacketErrorKind377::InvalidValue(
                    "rebuild region tile count",
                ));
            }
            if templates
                .iter()
                .any(|template| *template < -1 || *template > MAX_TEMPLATE)
            {
                return Err(PacketErrorKind377::InvalidValue("rebuild region template"));
            }
        }
        Ok(())
    }

    /// Rebuilt scene center in Java zone coordinates.
    pub fn zone_center(&self) -> (i32, i32) {
        match self {
            Self::Normal { zone_x, zone_z } | Self::Instance { zone_x, zone_z, .. } => {
                (*zone_x, *zone_z)
            }
        }
    }

    /// Java `sceneBaseTileX/Z = (sceneCenterZoneX/Z - 6) * 8`.
    pub fn scene_base_tile(&self) -> (i32, i32) {
        let (zone_x, zone_z) = self.zone_center();
        ((zone_x - 6) * 8, (zone_z - 6) * 8)
    }

    /// Whether Java marks the center as tutorial-island content.
    pub fn within_tutorial_island(&self) -> bool {
        let (zone_x, zone_z) = self.zone_center();
        let map_x = zone_x / 8;
        let map_z = zone_z / 8;
        ((map_x == 48 || map_x == 49) && map_z == 48) || (map_x == 48 && map_z == 148)
    }

    /// Whether Java's normal-rebuild map loader omits this map square in the
    /// tutorial island. This reports a selection only; it performs no lookup.
    pub fn excludes_tutorial_map_square(&self, map_x: i32, map_z: i32) -> bool {
        self.within_tutorial_island()
            && (map_z == 49
                || map_z == 149
                || map_z == 147
                || map_x == 50
                || (map_x == 49 && map_z == 47))
    }

    /// Borrow the flattened instance grid, or `None` for a normal rebuild.
    pub fn template_slots(&self) -> Option<&[i32]> {
        match self {
            Self::Normal { .. } => None,
            Self::Instance { templates, .. } => Some(templates),
        }
    }

    /// Iterate present destination-zone templates in Java's plane/x/z order.
    pub fn region_tiles(&self) -> impl Iterator<Item = MapRegionTileTemplate377> + '_ {
        let (zone_x, zone_z) = self.zone_center();
        let templates = self.template_slots().unwrap_or(&[]);
        (0..templates.len()).filter_map(move |index| {
            let packed = templates[index];
            if packed < 0 {
                return None;
            }
            let plane = index / (REGION_SIDE * REGION_SIDE);
            let within_plane = index % (REGION_SIDE * REGION_SIDE);
            let destination_zone_x = zone_x - 6 + (within_plane / REGION_SIDE) as i32;
            let destination_zone_z = zone_z - 6 + (within_plane % REGION_SIDE) as i32;
            Some(MapRegionTileTemplate377 {
                plane,
                destination_zone_x,
                destination_zone_z,
                destination_tile_x: destination_zone_x * 8,
                destination_tile_z: destination_zone_z * 8,
                source_level: (packed >> 24) & 0x3,
                source_zone_x: (packed >> 14) & 0x3ff,
                source_zone_z: (packed >> 3) & 0x7ff,
                rotation: (packed >> 1) & 0x3,
                packed,
            })
        })
    }

    /// Iterate the unique source map-square IDs in Java's first-appearance
    /// order. This intentionally uses the same bounded prior-scan strategy as
    /// Client.java:8212-8232 and does not allocate an auxiliary set.
    pub fn unique_map_squares(&self) -> impl Iterator<Item = i32> + '_ {
        let templates = self.template_slots().unwrap_or(&[]);
        let mut index = 0;
        std::iter::from_fn(move || {
            while index < templates.len() {
                let current = index;
                index += 1;
                let packed = templates[current];
                if packed < 0 {
                    continue;
                }
                let square = map_square_id(packed);
                let already_seen = templates[..current]
                    .iter()
                    .copied()
                    .any(|previous| previous >= 0 && map_square_id(previous) == square);
                if !already_seen {
                    return Some(square);
                }
            }
            None
        })
    }
}

/// Java Client.java:8216-8218, the source map-square ID for one present template.
fn map_square_id(packed: i32) -> i32 {
    let source_zone_x = (packed >> 14) & 0x3ff;
    let source_zone_z = (packed >> 3) & 0x7ff;
    ((source_zone_x / 8) << 8) + source_zone_z / 8
}

#[cfg(test)]
mod tests {
    use super::super::{PacketContext377, PacketErrorKind377, PacketReader377, ReferenceKind377};
    use super::{map_square_id, MapRegionTileTemplate377, Rebuild377};

    const PLANES: usize = 4;
    const SIDE: usize = 13;
    const SLOT_COUNT: usize = PLANES * SIDE * SIDE;
    const MAX_26: i32 = (1 << 26) - 1;

    struct EmptyContext;

    impl PacketContext377 for EmptyContext {
        fn contains(&self, _kind: ReferenceKind377, _id: i32) -> bool {
            false
        }

        fn inventory_capacity(&self, _component: i32) -> Option<usize> {
            None
        }
    }

    fn push_g2_alt2(bytes: &mut Vec<u8>, value: i32) {
        bytes.push((value >> 8) as u8);
        bytes.push((value as u8).wrapping_add(128));
    }

    fn push_bits(bytes: &mut Vec<u8>, bit_pos: &mut usize, value: u32, width: usize) {
        for bit in (0..width).rev() {
            let byte_index = *bit_pos / 8;
            if byte_index == bytes.len() {
                bytes.push(0);
            }
            if value & (1 << bit) != 0 {
                bytes[byte_index] |= 1 << (7 - *bit_pos % 8);
            }
            *bit_pos += 1;
        }
    }

    fn slot(plane: usize, zone_x: usize, zone_z: usize) -> usize {
        plane * SIDE * SIDE + zone_x * SIDE + zone_z
    }

    fn pack_template(level: u32, zone_x: u32, zone_z: u32, rotation: u32) -> i32 {
        (((level & 0x3) << 24)
            | ((zone_x & 0x3ff) << 14)
            | ((zone_z & 0x7ff) << 3)
            | ((rotation & 0x3) << 1)) as i32
    }

    fn region_payload(zone_x: i32, zone_z: i32, present: &[(usize, i32)]) -> Vec<u8> {
        let mut slots = vec![-1; SLOT_COUNT];
        for (index, template) in present {
            slots[*index] = *template;
        }

        let mut bit_bytes = Vec::new();
        let mut bit_pos = 0;
        for template in slots {
            if template < 0 {
                push_bits(&mut bit_bytes, &mut bit_pos, 0, 1);
            } else {
                push_bits(&mut bit_bytes, &mut bit_pos, 1, 1);
                push_bits(&mut bit_bytes, &mut bit_pos, template as u32, 26);
            }
        }

        let mut bytes = Vec::with_capacity(2 + bit_bytes.len() + 2);
        push_g2_alt2(&mut bytes, zone_x);
        bytes.extend_from_slice(&bit_bytes);
        push_g2_alt2(&mut bytes, zone_z);
        bytes
    }

    fn decode_region(bytes: &[u8]) -> Rebuild377 {
        let mut packet = PacketReader377::new(53, bytes);
        let operation = Rebuild377::decode(53, &mut packet).unwrap();
        packet.finish().unwrap();
        operation
    }

    #[test]
    fn normal_uses_java_z_then_transformed_x_words() {
        // g2(z) then p2_alt3(x): low x byte + 128 precedes high x byte.
        let bytes = [0xab, 0xcd, 0xf2, 0xf0];
        let mut packet = PacketReader377::new(222, &bytes);
        let operation = Rebuild377::decode(222, &mut packet).unwrap();
        assert_eq!(
            operation,
            Rebuild377::Normal {
                zone_x: 0xf072,
                zone_z: 0xabcd
            }
        );
        packet.finish().unwrap();
        assert_eq!(
            operation.scene_base_tile(),
            ((0xf072 - 6) * 8, (0xabcd - 6) * 8)
        );
        assert_eq!(operation.template_slots(), None);
        assert_eq!(operation.validate(&EmptyContext), Ok(()));
    }

    #[test]
    fn instance_decodes_plane_x_z_slots_and_26_bit_fields() {
        let packed_a = pack_template(0, 81, 161, 0);
        let packed_b = pack_template(3, 1023, 2047, 2);
        let packed_c = pack_template(2, 5, 9, 1);
        let present = [
            (slot(0, 0, 1), packed_a),
            (slot(1, 2, 3), packed_b),
            (slot(3, 12, 12), MAX_26),
            (slot(2, 4, 8), packed_c),
        ];
        let bytes = region_payload(0x92a1, 0x83b2, &present);
        let operation = decode_region(&bytes);
        assert_eq!(operation.zone_center(), (0x92a1, 0x83b2));
        assert_eq!(
            operation.scene_base_tile(),
            ((0x92a1 - 6) * 8, (0x83b2 - 6) * 8)
        );

        let slots = operation.template_slots().unwrap();
        assert_eq!(slots.len(), SLOT_COUNT);
        assert_eq!(slots[slot(0, 0, 1)], packed_a);
        assert_eq!(slots[slot(1, 2, 3)], packed_b);
        assert_eq!(slots[slot(2, 4, 8)], packed_c);
        assert_eq!(slots[slot(3, 12, 12)], MAX_26);
        assert_eq!(slots[slot(0, 0, 0)], -1);

        let tiles: Vec<MapRegionTileTemplate377> = operation.region_tiles().collect();
        assert_eq!(tiles.len(), 4);
        assert_eq!(
            tiles[0],
            MapRegionTileTemplate377 {
                plane: 0,
                destination_zone_x: 0x92a1 - 6,
                destination_zone_z: 0x83b2 - 5,
                destination_tile_x: (0x92a1 - 6) * 8,
                destination_tile_z: (0x83b2 - 5) * 8,
                source_level: 0,
                source_zone_x: 81,
                source_zone_z: 161,
                rotation: 0,
                packed: packed_a,
            }
        );
        assert_eq!(tiles[1].plane, 1);
        assert_eq!(tiles[1].source_level, 3);
        assert_eq!(tiles[1].source_zone_x, 1023);
        assert_eq!(tiles[1].source_zone_z, 2047);
        assert_eq!(tiles[1].rotation, 2);
        assert_eq!(tiles[2].plane, 2);
        assert_eq!(tiles[2].source_level, 2);
        assert_eq!(tiles[2].source_zone_x, 5);
        assert_eq!(tiles[2].source_zone_z, 9);
        assert_eq!(tiles[2].rotation, 1);
        assert_eq!(tiles[3].plane, 3);
        assert_eq!(tiles[3].source_level, 3);
        assert_eq!(tiles[3].source_zone_x, 1023);
        assert_eq!(tiles[3].source_zone_z, 2047);
        assert_eq!(tiles[3].rotation, 3);
        assert_eq!(tiles[3].packed, MAX_26);
        assert_eq!(operation.validate(&EmptyContext), Ok(()));
    }

    #[test]
    fn empty_and_all_present_masks_align_before_zone_z() {
        let empty = region_payload(1234, 2345, &[]);
        assert_eq!(empty.len(), 2 + (SLOT_COUNT).div_ceil(8) + 2);
        let empty_operation = decode_region(&empty);
        assert_eq!(empty_operation.region_tiles().count(), 0);
        assert_eq!(empty_operation.unique_map_squares().count(), 0);

        let dense_templates: Vec<_> = (0..SLOT_COUNT).map(|index| (index, MAX_26)).collect();
        let dense = region_payload(1234, 2345, &dense_templates);
        assert_eq!(dense.len(), 2 + (SLOT_COUNT * 27).div_ceil(8) + 2);
        let dense_operation = decode_region(&dense);
        assert_eq!(dense_operation.template_slots().unwrap().len(), SLOT_COUNT);
        assert!(dense_operation
            .template_slots()
            .unwrap()
            .iter()
            .all(|value| *value == MAX_26));
        assert_eq!(dense_operation.region_tiles().count(), SLOT_COUNT);
        assert_eq!(
            dense_operation.unique_map_squares().collect::<Vec<_>>(),
            [0x7fff]
        );
        assert_eq!(dense_operation.validate(&EmptyContext), Ok(()));
    }

    #[test]
    fn unique_map_squares_keep_first_plane_x_z_appearance() {
        let first = pack_template(0, 81, 161, 0); // map square (10, 20)
        let same_square = pack_template(3, 87, 167, 1); // same square, different template
        let second = pack_template(1, 88, 168, 2); // map square (11, 21)
        let third = pack_template(2, 80, 168, 3); // map square (10, 21)
        let operation = decode_region(&region_payload(
            700,
            900,
            &[
                (slot(0, 0, 0), first),
                (slot(0, 0, 1), same_square),
                (slot(0, 0, 2), second),
                (slot(1, 0, 0), third),
                (slot(3, 12, 12), second),
            ],
        ));
        assert_eq!(
            operation.unique_map_squares().collect::<Vec<_>>(),
            [0x0a14, 0x0b15, 0x0a15]
        );
        assert_eq!(map_square_id(MAX_26), 0x7fff);
    }

    #[test]
    fn tutorial_filter_and_scene_base_match_java_integer_rules() {
        let operation = Rebuild377::Normal {
            zone_x: 48 * 8,
            zone_z: 48 * 8,
        };
        assert!(operation.within_tutorial_island());
        for (map_x, map_z) in [(1, 49), (1, 149), (1, 147), (50, 1), (49, 47)] {
            assert!(operation.excludes_tutorial_map_square(map_x, map_z));
        }
        assert!(!operation.excludes_tutorial_map_square(49, 48));

        assert!(Rebuild377::Normal {
            zone_x: 49 * 8,
            zone_z: 48 * 8
        }
        .within_tutorial_island());
        assert!(Rebuild377::Normal {
            zone_x: 48 * 8,
            zone_z: 148 * 8
        }
        .within_tutorial_island());
        assert!(!Rebuild377::Normal {
            zone_x: 50 * 8,
            zone_z: 48 * 8
        }
        .within_tutorial_island());
        assert_eq!(
            operation.scene_base_tile(),
            ((48 * 8 - 6) * 8, (48 * 8 - 6) * 8)
        );
    }

    #[test]
    fn truncation_and_root_trailing_byte_checks_are_typed() {
        let short_normal = [0x00, 0x01, 0x00];
        let mut normal_packet = PacketReader377::new(222, &short_normal);
        assert!(matches!(
            Rebuild377::decode(222, &mut normal_packet)
                .unwrap_err()
                .kind,
            PacketErrorKind377::Truncated { .. }
        ));

        let short_region = [0, 128, 0];
        let mut region_packet = PacketReader377::new(53, &short_region);
        assert!(matches!(
            Rebuild377::decode(53, &mut region_packet).unwrap_err().kind,
            PacketErrorKind377::Truncated { .. }
        ));

        let normal_with_trailing = [0, 1, 128, 0, 0x55];
        let mut normal_packet = PacketReader377::new(222, &normal_with_trailing);
        Rebuild377::decode(222, &mut normal_packet).unwrap();
        assert_eq!(
            normal_packet.finish().unwrap_err().kind,
            PacketErrorKind377::TrailingBytes(1)
        );

        let mut region_with_trailing = region_payload(1, 2, &[]);
        region_with_trailing.push(0x55);
        let mut region_packet = PacketReader377::new(53, &region_with_trailing);
        Rebuild377::decode(53, &mut region_packet).unwrap();
        assert_eq!(
            region_packet.finish().unwrap_err().kind,
            PacketErrorKind377::TrailingBytes(1)
        );
    }

    #[test]
    fn validation_rejects_bad_instance_storage_counts_and_templates() {
        let wrong_count = Rebuild377::Instance {
            zone_x: 0,
            zone_z: 0,
            templates: vec![-1; SLOT_COUNT - 1].into_boxed_slice(),
        };
        assert_eq!(
            wrong_count.validate(&EmptyContext),
            Err(PacketErrorKind377::InvalidValue(
                "rebuild region tile count"
            ))
        );

        let mut invalid_template = vec![-1; SLOT_COUNT];
        invalid_template[0] = -2;
        let invalid = Rebuild377::Instance {
            zone_x: 0,
            zone_z: 0,
            templates: invalid_template.into_boxed_slice(),
        };
        assert_eq!(
            invalid.validate(&EmptyContext),
            Err(PacketErrorKind377::InvalidValue("rebuild region template"))
        );
    }
}
