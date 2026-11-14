//! Revision-377 zone origins and zone operations, decoded without client mutation.
//!
//! Java anchors: `Client.java:7884-7903,7961-7971,8313-8317,8587-8857`.
//! The `ServerGameZoneProt.ts` table assigns the eleven child opcodes; each
//! supported child has a dedicated server codec except `SOUND_AREA` (41), whose
//! protocol entry is present but which has no encoder in the engine codec tree.
//! Relative 0..7 tile offsets remain relative until a later apply slice.

use super::{
    require_reference, PacketContext377, PacketError377, PacketErrorKind377, PacketReader377,
    ReferenceKind377,
};

/// Zone frame and standalone operation opcodes implemented by this module.
pub const ALL_OPCODES_377: &[u8] = &[
    40, 75, 183, 203, 106, 142, 107, 121, 181, 41, 59, 152, 208, 88,
];

/// Relative coordinates packed into a zone operation.
#[derive(Debug, PartialEq, Eq)]
pub struct ZoneTileOffset377 {
    /// The packed coordinate's x nibble, in the range 0..=7.
    pub x: i32,
    /// The packed coordinate's z nibble, in the range 0..=7.
    pub z: i32,
}

/// A fully decoded zone frame. `origin` is present only for full-follows,
/// partial-follows, and partial-enclosed packets; standalone operations use
/// the currently established zone origin at apply time.
#[derive(Debug, PartialEq, Eq)]
pub struct ZoneFrame377 {
    origin: Option<(i32, i32)>,
    clear: bool,
    operations: ZoneOperations377,
}

impl ZoneFrame377 {
    /// Decode one zone protocol packet, or return `None` for a foreign opcode.
    pub(super) fn decode(
        opcode: u8,
        p: &mut PacketReader377<'_>,
    ) -> Result<Option<Self>, PacketError377> {
        let frame = match opcode {
            40 => {
                // Client.java:7884-7903, UPDATE_ZONE_FULL_FOLLOWS.
                let z = p.g1_alt3()?;
                let x = p.g1_alt2()?;
                Self {
                    origin: Some((x, z)),
                    clear: true,
                    operations: ZoneOperations377::Empty,
                }
            }
            75 => {
                // Client.java:8459-8465, UPDATE_ZONE_PARTIAL_FOLLOWS.
                let x = p.g1_alt2()?;
                let z = p.g1_alt1()?;
                Self {
                    origin: Some((x, z)),
                    clear: false,
                    operations: ZoneOperations377::Empty,
                }
            }
            183 => {
                // Client.java:7961-7971, UPDATE_ZONE_PARTIAL_ENCLOSED.
                let x = p.g1()?;
                let z = p.g1_alt1()?;
                let mut operations = Vec::new();
                while p.remaining() != 0 {
                    let child_opcode = p.g1()? as u8;
                    operations.push(ZoneOperation377::decode(child_opcode, p)?);
                }
                Self {
                    origin: Some((x, z)),
                    clear: false,
                    operations: ZoneOperations377::Enclosed(operations),
                }
            }
            203 | 106 | 142 | 107 | 121 | 181 | 41 | 59 | 152 | 208 | 88 => Self {
                // Client.java:8313-8317. The exact same parser serves enclosed children.
                origin: None,
                clear: false,
                operations: ZoneOperations377::Single(ZoneOperation377::decode(opcode, p)?),
            },
            _ => return Ok(None),
        };
        Ok(Some(frame))
    }

    /// The optional tile origin, as `(x, z)`.
    pub fn origin(&self) -> Option<(i32, i32)> {
        self.origin
    }

    /// Whether Java's full-follows packet clears its eight-by-eight zone.
    pub fn clear(&self) -> bool {
        self.clear
    }

    /// Borrow the operations without allocating for standalone operations.
    pub fn operations(&self) -> &[ZoneOperation377] {
        self.operations.as_slice()
    }
    /// Borrow the operation storage, including whether the frame is standalone or enclosed.
    pub fn operation_storage(&self) -> &ZoneOperations377 {
        &self.operations
    }

    /// Validate every decoded operation against a read-only session/cache view.
    pub fn validate(&self, context: &impl PacketContext377) -> Result<(), PacketErrorKind377> {
        for operation in self.operations() {
            operation.validate(context)?;
        }
        Ok(())
    }
}

/// Storage for zone operations; the single-operation form has no heap allocation.
#[derive(Debug, PartialEq, Eq)]
pub enum ZoneOperations377 {
    /// Full/partial follows packet with no zone child operations.
    Empty,
    /// One standalone child operation, stored inline.
    Single(ZoneOperation377),
    /// Operations from a partial-enclosed packet, in wire order.
    Enclosed(Vec<ZoneOperation377>),
}

impl ZoneOperations377 {
    /// Borrow the decoded operations in wire order.
    pub fn as_slice(&self) -> &[ZoneOperation377] {
        match self {
            Self::Empty => &[],
            Self::Single(operation) => std::slice::from_ref(operation),
            Self::Enclosed(operations) => operations,
        }
    }
}

/// One typed Java zone operation. Coordinates are offsets from the frame origin.
#[derive(Debug, PartialEq, Eq)]
pub enum ZoneOperation377 {
    /// Java `Client.java:8589-8649`, `LOC_MERGE` (203); extent endpoints are
    /// ordered as Java's signed-byte swap does before storing them.
    LocMerge {
        tile: ZoneTileOffset377,
        shape: i32,
        angle: i32,
        location: i32,
        min_x_offset: i32,
        max_x_offset: i32,
        min_z_offset: i32,
        max_z_offset: i32,
        player_slot: i32,
        start_cycle: i32,
        end_cycle: i32,
    },
    /// Java `Client.java:8650-8667`, `OBJ_REVEAL` (106); `65535` is the
    /// broadcast receiver sentinel compared against `localPid` by Java.
    ObjectReveal {
        tile: ZoneTileOffset377,
        count: i32,
        object: i32,
        receiver_id: i32,
    },
    /// Java `Client.java:8668-8716`, `LOC_ANIM` (142).
    LocAnim {
        tile: ZoneTileOffset377,
        shape: i32,
        angle: i32,
        sequence: i32,
    },
    /// Java `Client.java:8717-8733`, `OBJ_ADD` (107).
    ObjAdd {
        tile: ZoneTileOffset377,
        object: i32,
        count: i32,
    },
    /// Java `Client.java:8734-8753`, `OBJ_COUNT` (121); Java matches object
    /// IDs after masking off bit 15.
    ObjCount {
        tile: ZoneTileOffset377,
        object: i32,
        old_count: i32,
        new_count: i32,
    },
    /// Java `Client.java:8754-8777`, `MAP_PROJANIM` (181); `spotanim: None`
    /// represents Java's consumed 65535 no-projectile sentinel.
    MapProjAnim {
        tile: ZoneTileOffset377,
        destination_x_offset: i32,
        destination_z_offset: i32,
        target: i32,
        spotanim: Option<i32>,
        source_height: i32,
        destination_height: i32,
        start_delay: i32,
        end_delay: i32,
        peak: i32,
        arc: i32,
    },
    /// Java `Client.java:8778-8794`, `SOUND_AREA` (41).
    SoundArea {
        tile: ZoneTileOffset377,
        sound: i32,
        radius: i32,
        loops: i32,
    },
    /// Java `Client.java:8795-8808`, `MAP_ANIM` (59).
    MapAnim {
        tile: ZoneTileOffset377,
        spotanim: i32,
        height: i32,
        delay: i32,
    },
    /// Java `Client.java:8809-8821`, `LOC_ADD_CHANGE` (152).
    LocAddChange {
        tile: ZoneTileOffset377,
        shape: i32,
        angle: i32,
        location: i32,
    },
    /// Java `Client.java:8822-8842`, `OBJ_DEL` (208); Java compares IDs after
    /// masking off bit 15.
    ObjDel {
        tile: ZoneTileOffset377,
        object: i32,
    },
    /// Java `Client.java:8843-8855`, `LOC_DEL` (88); this uses the location
    /// removal sentinel rather than a location resource ID.
    LocDel {
        tile: ZoneTileOffset377,
        shape: i32,
        angle: i32,
    },
}

impl ZoneOperation377 {
    fn decode(opcode: u8, p: &mut PacketReader377<'_>) -> Result<Self, PacketError377> {
        let operation = match opcode {
            203 => {
                let location = p.g2()?;
                let info = p.g1()?;
                let (shape, angle) = shape_angle(info, p)?;
                let west = p.g1b_alt2()?;
                let tile = zone_tile(p.g1_alt1()?);
                let east = p.g1b_alt1()?;
                let end_cycle = p.g2_alt2()?;
                let player_slot = p.g2_alt1()?;
                let north = p.g1b()?;
                let south = p.g1b_alt1()?;
                let start_cycle = p.g2()?;
                if !(0..2048).contains(&player_slot) {
                    return Err(p.invalid("zone merge player slot"));
                }
                Self::LocMerge {
                    tile,
                    shape,
                    angle,
                    location,
                    min_x_offset: west.min(east),
                    max_x_offset: west.max(east),
                    min_z_offset: north.min(south),
                    max_z_offset: north.max(south),
                    player_slot,
                    start_cycle,
                    end_cycle,
                }
            }
            106 => Self::ObjectReveal {
                tile: zone_tile(p.g1_alt1()?),
                count: p.g2_alt3()?,
                object: p.g2_alt2()?,
                receiver_id: p.g2_alt2()?,
            },
            142 => {
                let sequence = p.g2()?;
                let info = p.g1_alt1()?;
                let (shape, angle) = shape_angle(info, p)?;
                // Client.java:8703-8705 changes layer-2 shape 11 to 10 before creating the animation.
                let shape = if shape == 11 { 10 } else { shape };
                Self::LocAnim {
                    tile: zone_tile(p.g1()?),
                    shape,
                    angle,
                    sequence,
                }
            }
            107 => Self::ObjAdd {
                object: p.g2()?,
                tile: zone_tile(p.g1_alt2()?),
                count: p.g2_alt2()?,
            },
            121 => {
                let tile = zone_tile(p.g1()?);
                let object = p.g2()? & 0x7fff;
                Self::ObjCount {
                    tile,
                    object,
                    old_count: p.g2()?,
                    new_count: p.g2()?,
                }
            }
            181 => {
                let tile = zone_tile(p.g1()?);
                let destination_x_offset = p.g1b()?;
                let destination_z_offset = p.g1b()?;
                let target = p.g2b()?;
                let spotanim_id = p.g2()?;
                let source_height = p.g1()? * 4;
                let destination_height = p.g1()? * 4;
                let start_delay = p.g2()?;
                let end_delay = p.g2()?;
                let peak = p.g1()?;
                let arc = p.g1()?;
                if spotanim_id != 65_535 && !(-2_048..=16_384).contains(&target) {
                    return Err(p.invalid("zone projectile target"));
                }
                Self::MapProjAnim {
                    tile,
                    destination_x_offset,
                    destination_z_offset,
                    target,
                    spotanim: (spotanim_id != 65_535).then_some(spotanim_id),
                    source_height,
                    destination_height,
                    start_delay,
                    end_delay,
                    peak,
                    arc,
                }
            }
            41 => {
                let tile = zone_tile(p.g1()?);
                let sound = p.g2()?;
                let packed = p.g1()?;
                Self::SoundArea {
                    tile,
                    sound,
                    radius: (packed >> 4) & 0xf,
                    loops: packed & 0x7,
                }
            }
            59 => Self::MapAnim {
                tile: zone_tile(p.g1()?),
                spotanim: p.g2()?,
                height: p.g1()?,
                delay: p.g2()?,
            },
            152 => {
                let info = p.g1_alt2()?;
                let (shape, angle) = shape_angle(info, p)?;
                let location = p.g2_alt3()?;
                let tile = zone_tile(p.g1_alt1()?);
                Self::LocAddChange {
                    tile,
                    shape,
                    angle,
                    location,
                }
            }
            208 => Self::ObjDel {
                object: p.g2_alt2()? & 0x7fff,
                tile: zone_tile(p.g1_alt1()?),
            },
            88 => {
                let tile = zone_tile(p.g1_alt3()?);
                let info = p.g1_alt3()?;
                let (shape, angle) = shape_angle(info, p)?;
                Self::LocDel { tile, shape, angle }
            }
            _ => return Err(p.error(PacketErrorKind377::UnknownOpcode)),
        };
        Ok(operation)
    }

    fn validate(&self, context: &impl PacketContext377) -> Result<(), PacketErrorKind377> {
        use ReferenceKind377 as R;
        match self {
            Self::LocMerge {
                location,
                player_slot,
                ..
            } => {
                require_reference(context, R::Location, *location)?;
                require_reference(context, R::Player, *player_slot)
            }
            Self::ObjectReveal {
                object,
                receiver_id,
                ..
            } => {
                require_reference(context, R::Object, *object)?;
                if *receiver_id == 65_535 {
                    Ok(())
                } else {
                    require_reference(context, R::Player, *receiver_id)
                }
            }
            Self::LocAnim { sequence, .. } => require_reference(context, R::Sequence, *sequence),
            Self::ObjAdd { object, .. }
            | Self::ObjCount { object, .. }
            | Self::ObjDel { object, .. } => require_reference(context, R::Object, *object),
            Self::MapProjAnim {
                target, spotanim, ..
            } => {
                if let Some(spotanim) = spotanim {
                    require_reference(context, R::Spotanim, *spotanim)?;
                    if *target > 0 {
                        require_reference(context, R::NpcSlot, *target - 1)
                    } else if *target < 0 {
                        require_reference(context, R::Player, -(*target) - 1)
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            Self::SoundArea { sound, .. } => require_reference(context, R::Sound, *sound),
            Self::MapAnim { spotanim, .. } => require_reference(context, R::Spotanim, *spotanim),
            Self::LocAddChange { location, .. } => {
                require_reference(context, R::Location, *location)
            }
            Self::LocDel { .. } => Ok(()),
        }
    }
}

fn zone_tile(packed: i32) -> ZoneTileOffset377 {
    ZoneTileOffset377 {
        x: (packed >> 4) & 0x7,
        z: packed & 0x7,
    }
}

fn shape_angle(info: i32, p: &PacketReader377<'_>) -> Result<(i32, i32), PacketError377> {
    let shape = info >> 2;
    if !(0..=22).contains(&shape) {
        return Err(p.invalid("zone location shape"));
    }
    Ok((shape, info & 0x3))
}
