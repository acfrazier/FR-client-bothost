//! Revision-377 inbound bytes -> decoded operation -> validated handoff.
//!
//! Primary: Client-Java 377 `client/Client.java:7216-8857`, and byte transforms
//! in `io/Packet.java:166-228,274-278,294-416`. Framing uses the stage-1 Java table.
//! No function here changes a client, loads a component, publishes a generation,
//! requests an asset, or calls a 274/289 handler. The standalone transport must
//! pass one complete payload to `decode_packet_377`, then call `validate` against
//! its immutable session/cache view before applying `into_operation`.
//!
//! Intentional malformed-input hardening: unknown opcodes, unknown enclosed zone
//! children, invalid indices/references and incomplete or trailing bytes return
//! typed errors before handoff. Java can mutate state before throwing, can ignore
//! an unknown enclosed child, and does not check every packet's final position.
//! Literal malformed-input parity is not claimed. Ignored fixed HINT_ARROW tail
//! bytes are consumed explicitly, not required to be zero. Text remains borrowed
//! bytes: Java `gjstr` uses the VM's default charset, not guaranteed UTF-8.
//!
//! Entity bitstreams (53/71/90/222 including rebuilds) belong to later slices and
//! fail with `DeferredOpcode`; they are never accepted as opaque operations.

#[path = "interface_packet_377.rs"]
pub mod interface_packet_377;
#[path = "zone_377.rs"]
pub mod zone_377;

mod sizes {
    include!("../io/server_prot_sizes_377.inc.rs");
}

use interface_packet_377::InterfaceOperation377;
use zone_377::ZoneFrame377;

/// The complete set of handlers in Java `readPacket`, minus the four deferred
/// entity/rebuild handlers. Zero-length table entries alone are not handlers.
pub const ALL_OPCODES_377: &[u8] = &[
    2, 3, 5, 6, 10, 13, 18, 21, 26, 29, 40, 41, 49, 50, 58, 59, 61, 63, 67, 75, 76, 78, 82, 88,
    106, 107, 109, 113, 115, 121, 125, 126, 128, 134, 135, 142, 148, 152, 156, 157, 158, 159, 162,
    166, 167, 174, 181, 182, 183, 186, 190, 199, 200, 201, 203, 206, 208, 216, 218, 219, 220, 226,
    232, 233, 238, 246, 249, 251, 252, 253, 255,
];
pub const DEFERRED_OPCODES_377: &[u8] = &[53, 71, 90, 222];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind377 {
    Component,
    Object,
    Npc,
    Sequence,
    /// Actor-array index, distinct from an NPC definition ID.
    NpcSlot,
    Model,
    Spotanim,
    Location,
    Sound,
    Midi,
    Varp,
    Stat,
    Player,
}

/// A read-only view of the actual revision-377 session/cache. Implementations
/// must not lazy-load components or mutate client/cache state during validation.
/// `contains` tests resource/index presence, not whether an actor is spawned.
/// Inventory capacity exists only for an inventory-bearing component.
pub trait PacketContext377 {
    fn contains(&self, kind: ReferenceKind377, id: i32) -> bool;
    fn inventory_capacity(&self, component: i32) -> Option<usize>;
}

pub fn require_reference(
    context: &impl PacketContext377,
    kind: ReferenceKind377,
    id: i32,
) -> Result<(), PacketErrorKind377> {
    if id >= 0 && context.contains(kind, id) {
        Ok(())
    } else {
        Err(PacketErrorKind377::InvalidReference { kind, id })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketErrorKind377 {
    UnknownOpcode,
    DeferredOpcode,
    WrongSize { expected: usize, actual: usize },
    FrameTooLarge { maximum: usize, actual: usize },
    Truncated { needed: usize, remaining: usize },
    UnterminatedString,
    TrailingBytes(usize),
    InvalidValue(&'static str),
    InvalidReference { kind: ReferenceKind377, id: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketError377 {
    pub opcode: u8,
    pub offset: usize,
    pub kind: PacketErrorKind377,
}

impl std::fmt::Display for PacketError377 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "377 packet {} at byte {}: {:?}",
            self.opcode, self.offset, self.kind
        )
    }
}
impl std::error::Error for PacketError377 {}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedPacket377<'a> {
    opcode: u8,
    size: usize,
    operation: Operation377<'a>,
}
impl<'a> DecodedPacket377<'a> {
    /// Inspection only. Only `validate` produces an applicable handoff.
    pub fn operation(&self) -> &Operation377<'a> {
        &self.operation
    }
    pub fn validate(
        self,
        context: &impl PacketContext377,
    ) -> Result<ValidatedPacket377<'a>, PacketError377> {
        self.operation
            .validate(context)
            .map_err(|kind| PacketError377 {
                opcode: self.opcode,
                offset: self.size,
                kind,
            })?;
        Ok(ValidatedPacket377 {
            operation: self.operation,
        })
    }
}

/// The operation is exposed for application only after the complete frame and
/// all resource checks succeed. Apply must occur against the same session view.
#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedPacket377<'a> {
    operation: Operation377<'a>,
}
impl<'a> ValidatedPacket377<'a> {
    pub fn operation(&self) -> &Operation377<'a> {
        &self.operation
    }
    pub fn into_operation(self) -> Operation377<'a> {
        self.operation
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InventoryEntry377 {
    /// Java's wire storage is object ID + 1. Zero is an empty slot.
    pub object_plus_one: i32,
    pub count: i32,
}
#[derive(Debug, PartialEq, Eq)]
pub struct InventorySlot377 {
    pub slot: i32,
    pub entry: InventoryEntry377,
}
#[derive(Debug, PartialEq, Eq)]
pub struct LastLogin377 {
    pub days_since_password_change: i32,
    pub field371: i32,
    pub unused_word: i32,
    pub field504: i32,
    pub current_day: i32,
    pub unread_messages: i32,
    pub previous_login_day: i32,
    pub members_days: i32,
    pub last_address: i32,
    pub recovery_change_day: i32,
    pub unused_byte: i32,
}
#[derive(Debug, PartialEq, Eq)]
pub enum HintArrow377 {
    Npc {
        slot: i32,
        padding: [u8; 3],
    },
    Player {
        slot: i32,
        padding: [u8; 3],
    },
    Tile {
        kind: i32,
        x: i32,
        z: i32,
        height: i32,
    },
    /// Java retains unknown types, including the server's 255 clear value.
    Other {
        kind: i32,
        padding: [u8; 5],
    },
}
#[derive(Debug, PartialEq, Eq)]
pub struct CameraTarget377 {
    pub x: i32,
    pub z: i32,
    pub height: i32,
    pub rate: i32,
    pub acceleration: i32,
}

/// Borrowed, fully checked ignore-list storage. No per-name allocation occurs.
#[derive(Debug, PartialEq, Eq)]
pub struct IgnoreNames377<'a>(&'a [u8]);
impl IgnoreNames377<'_> {
    pub fn iter(&self) -> impl ExactSizeIterator<Item = i64> + '_ {
        self.0
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| i64::from_be_bytes(*chunk))
    }
}

/// Decoded Java meaning, not a revision-289 opcode alias. Asset lookup, chat
/// filtering/decompression, redraws, timers, and conditional effects belong to
/// apply. In particular sound delay 65535 retains Java's special immediate path.
#[derive(Debug, PartialEq, Eq)]
pub enum Operation377<'a> {
    Interface(InterfaceOperation377<'a>),
    Zone(ZoneFrame377),
    InventoryFull {
        component: i32,
        entries: Vec<InventoryEntry377>,
    },
    InventoryPartial {
        component: i32,
        entries: Vec<InventorySlot377>,
    },
    InventoryStopTransmit {
        component: i32,
    },
    Varp {
        id: i32,
        value: i32,
    },
    VarpSync,
    UpdateStat {
        stat: i32,
        level: i32,
        experience: i32,
    },
    RunEnergy(i32),
    RunWeight(i32),
    ResetAnims,
    UpdatePid {
        members: i32,
        slot: i32,
    },
    Logout,
    LastLogin(LastLogin377),
    MessageGame(&'a [u8]),
    PrivateMessage {
        from: i64,
        message_id: i32,
        staff_level: i32,
        packed_body: &'a [u8],
    },
    IgnoreList(IgnoreNames377<'a>),
    Friend {
        name: i64,
        world: i32,
    },
    FriendListLoaded(i32),
    ChatFilter {
        public: i32,
        private: i32,
        trade: i32,
    },
    PlayerOption {
        index: i32,
        text: &'a [u8],
        priority: i32,
    },
    SynthSound {
        sound: i32,
        loops: i32,
        delay: i32,
    },
    MidiSong(i32),
    MidiJingle {
        song: i32,
        delay: i32,
    },
    MinimapToggle(i32),
    Multiway(i32),
    UnsetMapFlag,
    RebootTimer {
        ticks: i32,
    },
    Hint(HintArrow377),
    CameraLookAt(CameraTarget377),
    CameraMoveTo(CameraTarget377),
    CameraShake {
        axis: i32,
        jitter: i32,
        amplitude: i32,
        rate: i32,
    },
    CameraReset,
}

impl Operation377<'_> {
    fn validate(&self, context: &impl PacketContext377) -> Result<(), PacketErrorKind377> {
        use ReferenceKind377 as R;
        match self {
            Self::Interface(op) => op.validate(context),
            Self::Zone(frame) => frame.validate(context),
            Self::InventoryFull { component, entries } => {
                require_reference(context, R::Component, *component)?;
                let capacity = context
                    .inventory_capacity(*component)
                    .ok_or(PacketErrorKind377::InvalidValue("inventory component"))?;
                if entries.len() > capacity {
                    return Err(PacketErrorKind377::InvalidValue("full inventory capacity"));
                }
                for entry in entries {
                    validate_inventory_entry(context, entry)?;
                }
                Ok(())
            }
            Self::InventoryPartial { component, entries } => {
                require_reference(context, R::Component, *component)?;
                let capacity = context
                    .inventory_capacity(*component)
                    .ok_or(PacketErrorKind377::InvalidValue("inventory component"))?;
                // Java ignores out-of-range partial slots, after consuming them.
                for entry in entries {
                    if (entry.slot as usize) < capacity {
                        validate_inventory_entry(context, &entry.entry)?;
                    }
                }
                Ok(())
            }
            Self::InventoryStopTransmit { component } => {
                require_reference(context, R::Component, *component)?;
                context
                    .inventory_capacity(*component)
                    .ok_or(PacketErrorKind377::InvalidValue("inventory component"))?;
                Ok(())
            }
            Self::Varp { id, .. } => require_reference(context, R::Varp, *id),
            Self::UpdateStat { stat, .. } => require_reference(context, R::Stat, *stat),
            Self::UpdatePid { slot, .. } => require_reference(context, R::Player, *slot),
            Self::SynthSound { sound, delay, .. } if *delay != 65535 => {
                require_reference(context, R::Sound, *sound)
            }
            Self::MidiSong(song) if *song != -1 => require_reference(context, R::Midi, *song),
            Self::MidiJingle { song, .. } => require_reference(context, R::Midi, *song),
            // Hints refer to actor array slots, not loaded NPC definitions.
            Self::Hint(HintArrow377::Player { slot, .. }) => {
                require_reference(context, R::Player, *slot)
            }
            Self::Hint(HintArrow377::Npc { slot, .. }) => {
                require_reference(context, R::NpcSlot, *slot)
            }
            _ => Ok(()),
        }
    }
}
fn validate_inventory_entry(
    context: &impl PacketContext377,
    entry: &InventoryEntry377,
) -> Result<(), PacketErrorKind377> {
    if entry.object_plus_one != 0 {
        require_reference(context, ReferenceKind377::Object, entry.object_plus_one - 1)?;
    }
    Ok(())
}

/// Decode precisely one already-framed payload. Frame headers and ISAAC remain
/// owned by the existing standalone client transport (Java:7216-7266).
pub fn decode_packet_377(
    opcode: u8,
    payload: &[u8],
) -> Result<DecodedPacket377<'_>, PacketError377> {
    let error = |kind| PacketError377 {
        opcode,
        offset: 0,
        kind,
    };
    if DEFERRED_OPCODES_377.contains(&opcode) {
        return Err(error(PacketErrorKind377::DeferredOpcode));
    }
    if !ALL_OPCODES_377.contains(&opcode) {
        return Err(error(PacketErrorKind377::UnknownOpcode));
    }
    let length = sizes::SERVER_PROT_SIZES_377[opcode as usize];
    if length >= 0 && payload.len() != length as usize {
        return Err(error(PacketErrorKind377::WrongSize {
            expected: length as usize,
            actual: payload.len(),
        }));
    }
    let maximum = if length == -1 { 255 } else { 65535 };
    if payload.len() > maximum {
        return Err(error(PacketErrorKind377::FrameTooLarge {
            maximum,
            actual: payload.len(),
        }));
    }
    let mut p = PacketReader377 {
        opcode,
        data: payload,
        pos: 0,
    };
    let operation = if let Some(op) = InterfaceOperation377::decode(opcode, &mut p)? {
        Operation377::Interface(op)
    } else if let Some(frame) = ZoneFrame377::decode(opcode, &mut p)? {
        Operation377::Zone(frame)
    } else {
        decode_other(opcode, &mut p)?
    };
    p.finish()?;
    Ok(DecodedPacket377 {
        opcode,
        size: payload.len(),
        operation,
    })
}

fn decode_other<'a>(
    opcode: u8,
    p: &mut PacketReader377<'a>,
) -> Result<Operation377<'a>, PacketError377> {
    use Operation377 as O;
    Ok(match opcode {
        // Client.java:7303-7323. delay=65535 is an explicit alternate effect.
        26 => O::SynthSound {
            sound: p.g2()?,
            loops: p.g1()?,
            delay: p.g2()?,
        },
        // Client.java:7325-7340,7574-7589: small is signed, large mixed endian.
        182 => O::Varp {
            id: p.g2_alt2()?,
            value: p.g1b_alt3()?,
        },
        115 => {
            let value = p.g4_alt3()?;
            O::Varp {
                id: p.g2_alt1()?,
                value,
            }
        }
        113 => O::VarpSync, // Client.java:8527-8538: cache -> active, not zero reset.
        13 => O::ResetAnims, // Client.java:7342-7356.
        156 => O::MinimapToggle(p.g1()?), // Client.java:7358-7363.
        // Client.java:7408-7436. Song sentinel only; jingle delay is three bytes.
        220 => {
            let song = p.g2_alt3()?;
            O::MidiSong(if song == 65535 { -1 } else { song })
        }
        249 => O::MidiJingle {
            song: p.g2_alt1()?,
            delay: p.g3_alt3()?,
        },
        // Client.java:7462-7476: index outside 1..5 is a consumed Java no-op.
        157 => O::PlayerOption {
            index: p.g1_alt2()?,
            text: p.gjstr()?,
            priority: p.g1()?,
        },
        201 => O::ChatFilter {
            public: p.g1()?,
            private: p.g1()?,
            trade: p.g1()?,
        }, // 7488-7497.
        199 => O::Hint(decode_hint(p)?), // Client.java:7499-7536.
        167 => O::CameraLookAt(camera_target(p)?), // Client.java:7538-7565.
        3 => O::CameraMoveTo(camera_target(p)?), // Client.java:8349-8364.
        5 => O::Logout,                  // Client.java:7567-7572.
        // Client.java:7626-7642. Preserve even ignored/unnamed fields.
        76 => O::LastLogin(LastLogin377 {
            days_since_password_change: p.g2_alt1()?,
            field371: p.g2_alt3()?,
            unused_word: p.g2()?,
            field504: p.g2()?,
            current_day: p.g2_alt1()?,
            unread_messages: p.g2_alt2()?,
            previous_login_day: p.g2_alt2()?,
            members_days: p.g2()?,
            last_address: p.g4_alt1()?,
            recovery_change_day: p.g2_alt3()?,
            unused_byte: p.g1_alt1()?,
        }),
        63 => O::MessageGame(p.gjstr()?), // Client.java:7644-7692.
        174 => O::RunWeight(p.g2b()?),    // Client.java:7717-7725.
        233 => O::Multiway(p.g1()?),      // Client.java:7727-7732.
        61 => O::UnsetMapFlag,            // Client.java:7734-7739.
        // Client.java:7778-7791, camera arrays have exactly five slots.
        67 => {
            let axis = p.g1()?;
            let op = O::CameraShake {
                axis,
                jitter: p.g1()?,
                amplitude: p.g1()?,
                rate: p.g1()?,
            };
            if axis >= 5 {
                return Err(p.invalid("camera shake axis"));
            }
            op
        }
        // Client.java:7793-7812. Smart slot, untransformed object/count.
        134 => {
            let component = p.g2()?;
            let mut entries = Vec::new();
            while p.remaining() != 0 {
                entries.push(InventorySlot377 {
                    slot: p.gsmarts()?,
                    entry: inventory_entry(p, false)?,
                });
            }
            O::InventoryPartial { component, entries }
        }
        78 => O::Friend {
            name: p.g8()?,
            world: p.g1()?,
        }, // Client.java:7814-7863.
        // Client.java:7918-7959. Filtering and WordPack belong to apply.
        135 => O::PrivateMessage {
            from: p.g8()?,
            message_id: p.g4()?,
            staff_level: p.g1()?,
            packed_body: p.rest(),
        },
        49 => O::UpdateStat {
            stat: p.g1_alt2()?,
            level: p.g1()?,
            experience: p.g4()?,
        }, // 8047-8063.
        // Client.java:8065-8085. Validate enough bytes before reserving storage.
        206 => {
            let component = p.g2()?;
            let count = p.g2()? as usize;
            p.require(count * 3)?;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                entries.push(inventory_entry(p, true)?);
            }
            O::InventoryFull { component, entries }
        }
        190 => O::RebootTimer {
            ticks: p.g2_alt1()?,
        }, // Client.java:8306-8311, apply multiplies by 30.
        125 => O::RunEnergy(p.g1()?), // Client.java:8319-8327.
        // Client.java:8387-8395. Java divides by eight; reject discarded tails.
        226 => {
            if !p.remaining().is_multiple_of(8) || p.remaining() / 8 > 100 {
                return Err(p.invalid("ignore list capacity/alignment"));
            }
            O::IgnoreList(IgnoreNames377(p.rest()))
        }
        219 => O::InventoryStopTransmit {
            component: p.g2_alt1()?,
        }, // Client.java:8414-8424.
        148 => O::CameraReset, // Client.java:8441-8449.
        126 => O::UpdatePid {
            members: p.g1()?,
            slot: p.g2_alt1()?,
        }, // Client.java:8451-8457.
        251 => O::FriendListLoaded(p.g1()?), // Client.java:8501-8507.
        _ => return Err(p.error(PacketErrorKind377::UnknownOpcode)),
    })
}
fn inventory_entry(
    p: &mut PacketReader377<'_>,
    full: bool,
) -> Result<InventoryEntry377, PacketError377> {
    let object_plus_one = if full { p.g2_alt3()? } else { p.g2()? };
    let count = if full { p.g1_alt2()? } else { p.g1()? };
    let count = if count == 255 {
        if full {
            p.g4_alt1()?
        } else {
            p.g4()?
        }
    } else {
        count
    };
    Ok(InventoryEntry377 {
        object_plus_one,
        count,
    })
}
fn camera_target(p: &mut PacketReader377<'_>) -> Result<CameraTarget377, PacketError377> {
    Ok(CameraTarget377 {
        x: p.g1()?,
        z: p.g1()?,
        height: p.g2()?,
        rate: p.g1()?,
        acceleration: p.g1()?,
    })
}
fn decode_hint(p: &mut PacketReader377<'_>) -> Result<HintArrow377, PacketError377> {
    let kind = p.g1()?;
    Ok(match kind {
        1 | 10 => {
            let slot = p.g2()?;
            let padding = p.take::<3>()?;
            if kind == 1 {
                HintArrow377::Npc { slot, padding }
            } else {
                HintArrow377::Player { slot, padding }
            }
        }
        2..=6 => HintArrow377::Tile {
            kind,
            x: p.g2()?,
            z: p.g2()?,
            height: p.g1()?,
        },
        _ => HintArrow377::Other {
            kind,
            padding: p.take::<5>()?,
        },
    })
}

/// Borrowed network reader. Unlike shared `Packet`, it does not copy a frame or
/// panic at its end. All transformed methods use the original Java names.
pub struct PacketReader377<'a> {
    opcode: u8,
    data: &'a [u8],
    pos: usize,
}
impl<'a> PacketReader377<'a> {
    pub fn new(opcode: u8, data: &'a [u8]) -> Self {
        Self {
            opcode,
            data,
            pos: 0,
        }
    }
    pub fn position(&self) -> usize {
        self.pos
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn error(&self, kind: PacketErrorKind377) -> PacketError377 {
        PacketError377 {
            opcode: self.opcode,
            offset: self.pos,
            kind,
        }
    }
    pub fn invalid(&self, field: &'static str) -> PacketError377 {
        self.error(PacketErrorKind377::InvalidValue(field))
    }
    fn require(&self, needed: usize) -> Result<(), PacketError377> {
        if self.remaining() < needed {
            Err(self.error(PacketErrorKind377::Truncated {
                needed,
                remaining: self.remaining(),
            }))
        } else {
            Ok(())
        }
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], PacketError377> {
        self.require(N)?;
        let mut bytes = [0; N];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + N]);
        self.pos += N;
        Ok(bytes)
    }
    fn rest(&mut self) -> &'a [u8] {
        let rest = &self.data[self.pos..];
        self.pos = self.data.len();
        rest
    }
    fn finish(&self) -> Result<(), PacketError377> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(self.error(PacketErrorKind377::TrailingBytes(self.remaining())))
        }
    }
    pub fn g1(&mut self) -> Result<i32, PacketError377> {
        Ok(self.take::<1>()?[0] as i32)
    }
    pub fn g1b(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g1()? as i8 as i32)
    }
    pub fn g1_alt1(&mut self) -> Result<i32, PacketError377> {
        Ok((self.g1()? - 128) & 255)
    }
    pub fn g1_alt2(&mut self) -> Result<i32, PacketError377> {
        Ok(-self.g1()? & 255)
    }
    pub fn g1_alt3(&mut self) -> Result<i32, PacketError377> {
        Ok((128 - self.g1()?) & 255)
    }
    pub fn g1b_alt1(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g1_alt1()? as i8 as i32)
    }
    pub fn g1b_alt2(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g1_alt2()? as i8 as i32)
    }
    pub fn g1b_alt3(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g1_alt3()? as i8 as i32)
    }
    pub fn g2(&mut self) -> Result<i32, PacketError377> {
        Ok(u16::from_be_bytes(self.take()?) as i32)
    }
    pub fn g2b(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g2()? as i16 as i32)
    }
    pub fn g2_alt1(&mut self) -> Result<i32, PacketError377> {
        Ok(u16::from_le_bytes(self.take()?) as i32)
    }
    pub fn g2_alt2(&mut self) -> Result<i32, PacketError377> {
        let [a, b] = self.take()?;
        Ok(((a as i32) << 8) | b.wrapping_sub(128) as i32)
    }
    pub fn g2_alt3(&mut self) -> Result<i32, PacketError377> {
        let [a, b] = self.take()?;
        Ok(((b as i32) << 8) | a.wrapping_sub(128) as i32)
    }
    pub fn g2b_alt1(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g2_alt1()? as i16 as i32)
    }
    pub fn g2b_alt2(&mut self) -> Result<i32, PacketError377> {
        Ok(self.g2_alt2()? as i16 as i32)
    }
    pub fn g3_alt3(&mut self) -> Result<i32, PacketError377> {
        let [a, b, c] = self.take()?;
        Ok(((a as i32) << 8) | ((b as i32) << 16) | c as i32)
    }
    pub fn g4(&mut self) -> Result<i32, PacketError377> {
        Ok(i32::from_be_bytes(self.take()?))
    }
    pub fn g4_alt1(&mut self) -> Result<i32, PacketError377> {
        Ok(i32::from_le_bytes(self.take()?))
    }
    pub fn g4_alt3(&mut self) -> Result<i32, PacketError377> {
        let [a, b, c, d] = self.take()?;
        Ok(i32::from_be_bytes([b, a, d, c]))
    }
    pub fn g8(&mut self) -> Result<i64, PacketError377> {
        Ok(i64::from_be_bytes(self.take()?))
    }
    pub fn gsmarts(&mut self) -> Result<i32, PacketError377> {
        self.require(1)?;
        if self.data[self.pos] < 128 {
            self.g1()
        } else {
            Ok(self.g2()? - 32768)
        }
    }
    pub fn gjstr(&mut self) -> Result<&'a [u8], PacketError377> {
        let start = self.pos;
        let length = self.data[start..]
            .iter()
            .position(|b| *b == 10)
            .ok_or_else(|| self.error(PacketErrorKind377::UnterminatedString))?;
        self.pos += length + 1;
        Ok(&self.data[start..start + length])
    }
}
