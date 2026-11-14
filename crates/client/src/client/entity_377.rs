//! Shared wire-only data for revision-377 player and NPC information packets.
//!
//! Client-Java 377 `Client.java:198-210,351,9044-9195,9364-9481` stages actor
//! movement in bit mode and resolves byte-mode updates only after `accessBytes`.
//! Actor references here retain old-visible ordinals. Application resolves
//! those ordinals against the immutable pre-frame actor list. Face-entity slots
//! follow `Client.java:5332-5348`'s `<32768`/`>=32768` partition, bounded by
//! declared NPC and player array lengths.

use super::{PacketError377, PacketReader377};

pub(super) const PLAYER_SLOT_COUNT_377: usize = 2048;
pub(super) const NPC_SLOT_COUNT_377: usize = 16384;
pub(super) const PLAYER_NEW_INDEX_SENTINEL_377: i32 = 2047;
pub(super) const NPC_NEW_INDEX_SENTINEL_377: i32 = 16383;

/// One movement command preserved from the revision-377 actor bit stream.
#[derive(Debug, PartialEq, Eq)]
pub enum ActorMovement377 {
    /// One walk direction, or two run-step directions in wire order.
    Step {
        first_direction: i32,
        second_direction: Option<i32>,
    },
    /// A local-player absolute move. `plane` is present only for the player's
    /// local teleport operation.
    Teleport {
        plane: Option<i32>,
        x: i32,
        z: i32,
        jump: bool,
    },
    /// A newly visible actor's signed offset from the local player's tile.
    Offset { dx: i32, dz: i32, jump: bool },
}

/// An actor selected by a decoded update block.
///
/// `OldVisible` is an ordinal in the pre-frame visible list. `New` is a player
/// or NPC slot index, as determined by the enclosing packet operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorReference377 {
    LocalPlayer,
    OldVisible(usize),
    New(i32),
}

/// Target interpretation for Java `ClientEntity.field1156`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorTarget377 {
    NpcSlot(usize),
    PlayerSlot(usize),
}

/// The decoded face-entity value, including Java's `65535` clear sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorTargetField377 {
    /// The original unsigned wire word.
    pub wire_value: i32,
    /// `None` represents the Java `-1` target after normalization.
    pub target: Option<ActorTarget377>,
}

/// An old-visible actor's update in its original pre-frame list position.
#[derive(Debug, PartialEq, Eq)]
pub struct OldVisibleActor377 {
    /// Movement is absent for idle, mask-only, and removed actors.
    pub movement: Option<ActorMovement377>,
    /// Java op 3 removes this actor from the new visible list.
    pub removed: bool,
}

/// Transformation applied by Java's `gdata_alt1` or `gdata_alt2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataTransform377 {
    /// Java copies the wire bytes into the destination in reverse order.
    Reverse,
    /// Java stores each wire byte minus 128, with byte wrapping.
    Subtract128,
}

/// A borrowed transformed byte block. The original wire bytes remain intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformedBytes377<'a> {
    /// Encoded bytes as they appeared in the packet.
    pub wire: &'a [u8],
    /// The transform Java applies before the next decoding stage.
    pub transform: DataTransform377,
}
impl TransformedBytes377<'_> {
    /// Iterate over bytes after Java's data-block transform without copying.
    pub fn decoded_iter(&self) -> TransformedBytesIter377<'_> {
        TransformedBytesIter377 {
            bytes: self.wire,
            transform: self.transform,
            index: 0,
        }
    }
}

/// Allocation-free iterator over one Java-transformed byte block.
pub struct TransformedBytesIter377<'a> {
    bytes: &'a [u8],
    transform: DataTransform377,
    index: usize,
}
impl Iterator for TransformedBytesIter377<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let wire = *self.bytes.get(self.index)?;
        let decoded = match self.transform {
            DataTransform377::Reverse => self.bytes[self.bytes.len() - self.index - 1],
            DataTransform377::Subtract128 => wire.wrapping_sub(128),
        };
        self.index += 1;
        Some(decoded)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.bytes.len() - self.index;
        (remaining, Some(remaining))
    }
}
impl ExactSizeIterator for TransformedBytesIter377<'_> {}

/// Parse Java's encoded face-entity target after applying its `65535` sentinel.
///
/// Java indexes `npcs[value]` below 32768 and `players[value - 32768]` at or
/// above 32768. Reject values that would index beyond the 16384- and 2048-slot
/// arrays. Java's actor update loop ignores a valid slot when it is null.
pub(super) fn decode_actor_target(
    wire_value: i32,
    packet: &PacketReader377<'_>,
) -> Result<ActorTargetField377, PacketError377> {
    let target = if wire_value == 65535 {
        None
    } else if wire_value < 32768 {
        let index = wire_value as usize;
        if index >= NPC_SLOT_COUNT_377 {
            return Err(packet.invalid("face_entity_npc_slot"));
        }
        Some(ActorTarget377::NpcSlot(index))
    } else {
        let index = (wire_value - 32768) as usize;
        if index >= PLAYER_SLOT_COUNT_377 {
            return Err(packet.invalid("face_entity_player_slot"));
        }
        Some(ActorTarget377::PlayerSlot(index))
    };
    Ok(ActorTargetField377 { wire_value, target })
}
/// Check a public face-entity value against Java's array partition and sentinel.
pub(super) fn actor_target_field_matches(field: ActorTargetField377) -> bool {
    let expected = if field.wire_value == 65535 {
        None
    } else if field.wire_value >= 0 && field.wire_value < 32768 {
        let index = field.wire_value as usize;
        if index >= NPC_SLOT_COUNT_377 {
            return false;
        }
        Some(ActorTarget377::NpcSlot(index))
    } else if field.wire_value >= 32768 {
        let index = (field.wire_value - 32768) as usize;
        if index >= PLAYER_SLOT_COUNT_377 {
            return false;
        }
        Some(ActorTarget377::PlayerSlot(index))
    } else {
        return false;
    };
    field.target == expected
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::super::{PacketContext377, ReferenceKind377};
    use std::vec::Vec;

    /// MSB-first bit writer for in-module protocol tests.
    pub struct BitWriter377 {
        bytes: Vec<u8>,
        bit_position: usize,
    }
    impl BitWriter377 {
        pub fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit_position: 0,
            }
        }
        pub fn write(&mut self, width: u8, value: i32) {
            for bit in (0..width).rev() {
                let byte = self.bit_position / 8;
                if byte == self.bytes.len() {
                    self.bytes.push(0);
                }
                let shift = 7 - self.bit_position % 8;
                self.bytes[byte] |= (((value as u32 >> bit) & 1) as u8) << shift;
                self.bit_position += 1;
            }
        }
        pub fn align(&mut self) {
            if !self.bit_position.is_multiple_of(8) {
                self.bit_position = self.bit_position.div_ceil(8) * 8;
                self.bytes.resize(self.bit_position / 8, 0);
            }
        }
        pub fn finish(self) -> Vec<u8> {
            self.bytes
        }
    }

    /// Read-only validation context for packet unit tests.
    #[derive(Default)]
    pub struct TestContext377 {
        pub visible_players: Option<usize>,
        pub visible_npcs: Option<usize>,
        references: Vec<(ReferenceKind377, i32)>,
    }
    impl TestContext377 {
        pub fn allow(&mut self, kind: ReferenceKind377, id: i32) {
            self.references.push((kind, id));
        }
    }
    impl PacketContext377 for TestContext377 {
        fn contains(&self, kind: ReferenceKind377, id: i32) -> bool {
            self.references.contains(&(kind, id))
        }
        fn inventory_capacity(&self, _component: i32) -> Option<usize> {
            None
        }
        fn visible_player_count(&self) -> Option<usize> {
            self.visible_players
        }
        fn visible_npc_count(&self) -> Option<usize> {
            self.visible_npcs
        }
    }
}
