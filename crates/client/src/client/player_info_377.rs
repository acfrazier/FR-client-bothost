//! Revision-377 opcode 90 player-info decoder.
//!
//! Java anchors: `Client.java:9015-9362`, `ClientPlayer.java:332-422`, and
//! `Packet.java:310-431`. The decoder stages movement and byte-mode updates;
//! it does not mutate actor state. Player appearance keeps its complete wire
//! block and also parses every Java `method574` field. Application still owns
//! base-37 display-name conversion and body-colour palette normalization.
//!
//! Engine source cross-check: `src/network/rsbuf/info.ts:21-263` writes player
//! bit/mask order, while `src/network/rsbuf/messages.ts:9-220` provides its byte
//! encoders. `src/network/game/server/codec/PlayerInfoEncoder.ts:9-11` only
//! copies the completed bytes.
//! Malformed masks, actor slots, truncated fields, and trailing frame or
//! appearance bytes are rejected before the operation is handed off; Java
//! would discover some of these only after mutating client state.

use super::entity_377::{
    actor_target_field_matches, decode_actor_target, ActorMovement377, ActorReference377,
    ActorTargetField377, DataTransform377, OldVisibleActor377, TransformedBytes377,
    PLAYER_NEW_INDEX_SENTINEL_377,
};
use super::{
    require_reference, PacketContext377, PacketError377, PacketErrorKind377, PacketReader377,
    ReferenceKind377,
};
use std::ops::Range;

const MASK_ANIMATION: i32 = 0x008;
const MASK_SAY: i32 = 0x010;
const MASK_EXACT_MOVE: i32 = 0x100;
const MASK_FACE_ENTITY: i32 = 0x001;
const MASK_FACE_COORDINATE: i32 = 0x002;
const MASK_SPOT_ANIMATION: i32 = 0x200;
const MASK_APPEARANCE: i32 = 0x004;
const MASK_DAMAGE: i32 = 0x400;
const MASK_CHAT: i32 = 0x040;
const MASK_DAMAGE2: i32 = 0x080;
const MASK_EXTENDED: i32 = 0x020;
const MASK_KNOWN: i32 = MASK_ANIMATION
    | MASK_SAY
    | MASK_EXACT_MOVE
    | MASK_FACE_ENTITY
    | MASK_FACE_COORDINATE
    | MASK_SPOT_ANIMATION
    | MASK_APPEARANCE
    | MASK_DAMAGE
    | MASK_CHAT
    | MASK_DAMAGE2;

/// One decoded opcode-90 frame. Old-visible references use pre-frame ordinals.
#[derive(Debug, PartialEq, Eq)]
pub struct PlayerInfo377<'a> {
    /// Player movement for the separate local-player movement header.
    pub local_movement: Option<ActorMovement377>,
    /// Number of old-visible players that the frame describes.
    ///
    /// Java removes old-list entries after this ordinal when the wire count is
    /// smaller than the actual pre-frame list.
    pub old_visible_count: usize,
    /// One entry for each wire old-visible ordinal, including explicit removals.
    pub old_visible: Vec<OldVisibleActor377>,
    /// Newly visible player slots, in packet order.
    pub additions: Vec<PlayerAddition377>,
    /// Mask-bearing actors, in Java's local/old/new update order.
    pub updates: Vec<PlayerActorUpdate377>,
    /// Flat mask-block storage. Each update's `blocks` range selects its blocks.
    pub blocks: Vec<PlayerUpdateBlock377<'a>>,
}

/// A newly visible player and its signed local-tile offset.
#[derive(Debug, PartialEq, Eq)]
pub struct PlayerAddition377 {
    /// Player array slot. 2047 is the wire stop sentinel and is never an entry.
    pub index: i32,
    /// Signed offsets use Java's five-bit two's-complement rule.
    pub movement: ActorMovement377,
}

/// The contiguous mask blocks for one player actor.
#[derive(Debug, PartialEq, Eq)]
pub struct PlayerActorUpdate377 {
    pub actor: ActorReference377,
    pub blocks: Range<usize>,
}

/// One Java player update block, decoded in Java's mask order.
#[derive(Debug, PartialEq, Eq)]
pub enum PlayerUpdateBlock377<'a> {
    Animation {
        sequence: Option<i32>,
        delay: i32,
    },
    Say {
        text: &'a [u8],
    },
    ExactMove {
        start_x: i32,
        start_z: i32,
        end_x: i32,
        end_z: i32,
        start_delay: i32,
        end_delay: i32,
        direction: i32,
    },
    FaceEntity(ActorTargetField377),
    FaceCoordinate {
        x: i32,
        z: i32,
    },
    SpotAnimation {
        animation: Option<i32>,
        height: i32,
        delay: i32,
    },
    Appearance(PlayerAppearance377<'a>),
    Damage {
        amount: i32,
        kind: i32,
        current_hitpoints: i32,
        base_hitpoints: i32,
    },
    Chat {
        colour: i32,
        effect: i32,
        rights: i32,
        packed: TransformedBytes377<'a>,
    },
    Damage2 {
        amount: i32,
        kind: i32,
        current_hitpoints: i32,
        base_hitpoints: i32,
    },
}

/// Fully parsed ClientPlayer `method574` appearance data.
#[derive(Debug, PartialEq, Eq)]
pub struct PlayerAppearance377<'a> {
    /// Complete gdata_alt1 wire block, retained for Java-equivalent caching.
    pub raw: TransformedBytes377<'a>,
    pub gender: i32,
    pub head_icon: i32,
    pub skull_icon: i32,
    /// `None` means Java's transmog branch did not encode this equipment slot.
    /// Values 256..511 require an identity kit. Values >=512 require an object
    /// definition before the renderer can use them.
    pub equipment: [Option<i32>; 12],
    /// Present when equipment slot zero contains Java's 65535 transmog marker.
    pub npc_transform: Option<i32>,
    /// Raw palette indices. Java clamps them using DESIGN_BODY_COLOUR on apply.
    pub body_colours: [i32; 5],
    /// Java's seven sequence fields. 65535 is represented as `None`.
    pub sequences: [Option<i32>; 7],
    /// The unformatted base-37 name value from the appearance block.
    pub username_base37: i64,
    pub combat_level: i32,
    pub skill_level: i32,
}

impl<'a> PlayerInfo377<'a> {
    /// Decode one complete opcode-90 payload without changing client state.
    pub(super) fn decode(p: &mut PacketReader377<'a>) -> Result<Self, PacketError377> {
        let mut bits = p.bits();
        let total_bits = bits.bit_position() + bits.remaining_bits();
        let mut local_movement = None;
        let mut updates = Vec::new();

        if bits.gbit(1)? != 0 {
            match bits.gbit(2)? {
                0 => updates.push(PlayerActorUpdate377 {
                    actor: ActorReference377::LocalPlayer,
                    blocks: 0..0,
                }),
                1 => {
                    let direction = bits.gbit(3)?;
                    local_movement = Some(ActorMovement377::Step {
                        first_direction: direction,
                        second_direction: None,
                    });
                    if bits.gbit(1)? != 0 {
                        updates.push(PlayerActorUpdate377 {
                            actor: ActorReference377::LocalPlayer,
                            blocks: 0..0,
                        });
                    }
                }
                2 => {
                    let first_direction = bits.gbit(3)?;
                    let second_direction = bits.gbit(3)?;
                    local_movement = Some(ActorMovement377::Step {
                        first_direction,
                        second_direction: Some(second_direction),
                    });
                    if bits.gbit(1)? != 0 {
                        updates.push(PlayerActorUpdate377 {
                            actor: ActorReference377::LocalPlayer,
                            blocks: 0..0,
                        });
                    }
                }
                3 => {
                    let jump = bits.gbit(1)? != 0;
                    let plane = bits.gbit(2)?;
                    let z = bits.gbit(7)?;
                    let x = bits.gbit(7)?;
                    local_movement = Some(ActorMovement377::Teleport {
                        plane: Some(plane),
                        x,
                        z,
                        jump,
                    });
                    if bits.gbit(1)? != 0 {
                        updates.push(PlayerActorUpdate377 {
                            actor: ActorReference377::LocalPlayer,
                            blocks: 0..0,
                        });
                    }
                }
                _ => unreachable!("two-bit player movement value"),
            }
        }

        let old_visible_count = bits.gbit(8)? as usize;
        let mut old_visible = Vec::with_capacity(old_visible_count);
        for ordinal in 0..old_visible_count {
            if bits.gbit(1)? == 0 {
                old_visible.push(OldVisibleActor377 {
                    movement: None,
                    removed: false,
                });
                continue;
            }
            match bits.gbit(2)? {
                0 => {
                    old_visible.push(OldVisibleActor377 {
                        movement: None,
                        removed: false,
                    });
                    updates.push(PlayerActorUpdate377 {
                        actor: ActorReference377::OldVisible(ordinal),
                        blocks: 0..0,
                    });
                }
                1 => {
                    let direction = bits.gbit(3)?;
                    let update = bits.gbit(1)? != 0;
                    old_visible.push(OldVisibleActor377 {
                        movement: Some(ActorMovement377::Step {
                            first_direction: direction,
                            second_direction: None,
                        }),
                        removed: false,
                    });
                    if update {
                        updates.push(PlayerActorUpdate377 {
                            actor: ActorReference377::OldVisible(ordinal),
                            blocks: 0..0,
                        });
                    }
                }
                2 => {
                    let first_direction = bits.gbit(3)?;
                    let second_direction = bits.gbit(3)?;
                    let update = bits.gbit(1)? != 0;
                    old_visible.push(OldVisibleActor377 {
                        movement: Some(ActorMovement377::Step {
                            first_direction,
                            second_direction: Some(second_direction),
                        }),
                        removed: false,
                    });
                    if update {
                        updates.push(PlayerActorUpdate377 {
                            actor: ActorReference377::OldVisible(ordinal),
                            blocks: 0..0,
                        });
                    }
                }
                3 => old_visible.push(OldVisibleActor377 {
                    movement: None,
                    removed: true,
                }),
                _ => unreachable!("two-bit old-player movement value"),
            }
        }

        // Preserve Java's exact guard: bitPos + 10 < psize * 8.
        let mut additions = Vec::new();
        let mut seen_additions = [0u64; 32];
        while bits.bit_position() + 10 < total_bits {
            let index = bits.gbit(11)?;
            if index == PLAYER_NEW_INDEX_SENTINEL_377 {
                break;
            }
            let slot = index as usize;
            let word = slot / 64;
            let bit = 1u64 << (slot % 64);
            if seen_additions[word] & bit != 0 {
                return Err(p.invalid("duplicate_new_player_index"));
            }
            seen_additions[word] |= bit;
            let dx = signed_five(bits.gbit(5)?);
            let update = bits.gbit(1)? != 0;
            let jump = bits.gbit(1)? != 0;
            let dz = signed_five(bits.gbit(5)?);
            additions.push(PlayerAddition377 {
                index,
                movement: ActorMovement377::Offset { dx, dz, jump },
            });
            if update {
                updates.push(PlayerActorUpdate377 {
                    actor: ActorReference377::New(index),
                    blocks: 0..0,
                });
            }
        }
        bits.access_bytes();

        let mut blocks = Vec::new();
        for update in &mut updates {
            let start = blocks.len();
            let mask = player_mask(p)?;
            decode_player_blocks(mask, p, &mut blocks)?;
            update.blocks = start..blocks.len();
        }
        finish_payload(p)?;

        Ok(Self {
            local_movement,
            old_visible_count,
            old_visible,
            additions,
            updates,
            blocks,
        })
    }

    /// Validate old-visible membership and every referenced cache resource.
    pub(super) fn validate(
        &self,
        context: &impl PacketContext377,
    ) -> Result<(), PacketErrorKind377> {
        use ReferenceKind377 as R;

        let visible_count = context
            .visible_player_count()
            .ok_or(PacketErrorKind377::InvalidValue("visible_player_count"))?;
        if self.old_visible_count != self.old_visible.len()
            || self.old_visible_count > visible_count
        {
            return Err(PacketErrorKind377::InvalidValue("old_visible_player_count"));
        }

        let mut additions = [0u64; 32];
        for addition in &self.additions {
            if addition.index < 0 || addition.index >= PLAYER_NEW_INDEX_SENTINEL_377 {
                return Err(PacketErrorKind377::InvalidValue("new_player_index"));
            }
            let slot = addition.index as usize;
            let word = slot / 64;
            let bit = 1u64 << (slot % 64);
            if additions[word] & bit != 0 {
                return Err(PacketErrorKind377::InvalidValue(
                    "duplicate_new_player_index",
                ));
            }
            additions[word] |= bit;
        }

        let mut old_updates = [0u64; 4];
        let mut new_updates = [0u64; 32];
        let mut local_update = false;
        let mut next_block = 0;
        for update in &self.updates {
            if update.blocks.start != next_block
                || update.blocks.start > update.blocks.end
                || update.blocks.end > self.blocks.len()
            {
                return Err(PacketErrorKind377::InvalidValue(
                    "player_update_block_range",
                ));
            }
            next_block = update.blocks.end;
            match update.actor {
                ActorReference377::LocalPlayer if !local_update => local_update = true,
                ActorReference377::LocalPlayer => {
                    return Err(PacketErrorKind377::InvalidValue(
                        "duplicate_local_player_update",
                    ));
                }
                ActorReference377::OldVisible(ordinal)
                    if ordinal < self.old_visible_count
                        && !self.old_visible[ordinal].removed
                        && old_updates[ordinal / 64] & (1u64 << (ordinal % 64)) == 0 =>
                {
                    old_updates[ordinal / 64] |= 1u64 << (ordinal % 64);
                }
                ActorReference377::OldVisible(_) => {
                    return Err(PacketErrorKind377::InvalidValue(
                        "player_update_old_ordinal",
                    ));
                }
                ActorReference377::New(index)
                    if index >= 0
                        && (index as usize) < PLAYER_NEW_INDEX_SENTINEL_377 as usize
                        && additions[index as usize / 64] & (1u64 << (index as usize % 64))
                            != 0
                        && new_updates[index as usize / 64] & (1u64 << (index as usize % 64))
                            == 0 =>
                {
                    new_updates[index as usize / 64] |= 1u64 << (index as usize % 64);
                }
                ActorReference377::New(_) => {
                    return Err(PacketErrorKind377::InvalidValue("player_update_new_index"));
                }
            }
            for block in &self.blocks[update.blocks.clone()] {
                match block {
                    PlayerUpdateBlock377::Animation {
                        sequence: Some(sequence),
                        ..
                    } => require_reference(context, R::Sequence, *sequence)?,
                    PlayerUpdateBlock377::SpotAnimation {
                        animation: Some(animation),
                        ..
                    } => require_reference(context, R::Spotanim, *animation)?,
                    PlayerUpdateBlock377::Appearance(appearance) => {
                        if let Some(npc) = appearance.npc_transform {
                            require_reference(context, R::Npc, npc)?;
                        }
                        for (slot, part) in appearance.equipment.iter().enumerate() {
                            let Some(part) = part else {
                                continue;
                            };
                            if *part == 65535 && slot == 0 && appearance.npc_transform.is_some() {
                                continue;
                            }
                            if (256..512).contains(part) {
                                require_reference(context, R::IdentityKit, *part - 256)?;
                            } else if *part >= 512 {
                                require_reference(context, R::Object, *part - 512)?;
                            }
                        }
                        for sequence in appearance.sequences.iter().flatten() {
                            require_reference(context, R::Sequence, *sequence)?;
                        }
                    }
                    PlayerUpdateBlock377::FaceEntity(target)
                        if !actor_target_field_matches(*target) =>
                    {
                        return Err(PacketErrorKind377::InvalidValue("face_entity_target"));
                    }
                    _ => {}
                }
            }
        }
        if next_block != self.blocks.len() {
            return Err(PacketErrorKind377::InvalidValue(
                "unassigned_player_update_blocks",
            ));
        }
        Ok(())
    }
}

fn signed_five(value: i32) -> i32 {
    if value > 15 {
        value - 32
    } else {
        value
    }
}

fn player_mask(p: &mut PacketReader377<'_>) -> Result<i32, PacketError377> {
    let mut flags = p.g1()?;
    if flags & MASK_EXTENDED != 0 {
        flags |= p.g1()? << 8;
    }
    if flags & !(MASK_KNOWN | MASK_EXTENDED) != 0 {
        return Err(p.invalid("unknown_player_update_mask"));
    }
    Ok(flags)
}

fn decode_player_blocks<'a>(
    mask: i32,
    p: &mut PacketReader377<'a>,
    blocks: &mut Vec<PlayerUpdateBlock377<'a>>,
) -> Result<(), PacketError377> {
    if mask & MASK_ANIMATION != 0 {
        blocks.push(PlayerUpdateBlock377::Animation {
            sequence: optional_id(p.g2()?),
            delay: p.g1_alt3()?,
        });
    }
    if mask & MASK_SAY != 0 {
        blocks.push(PlayerUpdateBlock377::Say { text: p.gjstr()? });
    }
    if mask & MASK_EXACT_MOVE != 0 {
        blocks.push(PlayerUpdateBlock377::ExactMove {
            start_x: p.g1_alt1()?,
            start_z: p.g1_alt2()?,
            end_x: p.g1_alt3()?,
            end_z: p.g1()?,
            start_delay: p.g2()?,
            end_delay: p.g2_alt2()?,
            direction: p.g1()?,
        });
    }
    if mask & MASK_FACE_ENTITY != 0 {
        let wire_value = p.g2_alt2()?;
        blocks.push(PlayerUpdateBlock377::FaceEntity(decode_actor_target(
            wire_value, p,
        )?));
    }
    if mask & MASK_FACE_COORDINATE != 0 {
        blocks.push(PlayerUpdateBlock377::FaceCoordinate {
            x: p.g2()?,
            z: p.g2()?,
        });
    }
    if mask & MASK_SPOT_ANIMATION != 0 {
        let animation = optional_id(p.g2_alt2()?);
        let packed = p.g4_alt2()?;
        blocks.push(PlayerUpdateBlock377::SpotAnimation {
            animation,
            height: packed >> 16,
            delay: packed & 0xFFFF,
        });
    }
    if mask & MASK_APPEARANCE != 0 {
        let length = p.g1()? as usize;
        let start = p.position();
        let wire = p.take_bytes(length)?;
        let raw = TransformedBytes377 {
            wire,
            transform: DataTransform377::Reverse,
        };
        blocks.push(PlayerUpdateBlock377::Appearance(parse_appearance(
            raw, p, start,
        )?));
    }
    if mask & MASK_DAMAGE != 0 {
        blocks.push(PlayerUpdateBlock377::Damage {
            amount: p.g1_alt1()?,
            kind: p.g1_alt3()?,
            current_hitpoints: p.g1_alt2()?,
            base_hitpoints: p.g1()?,
        });
    }
    if mask & MASK_CHAT != 0 {
        let colour_effect = p.g2()?;
        let rights = p.g1_alt2()?;
        let length = p.g1_alt1()? as usize;
        let wire = p.take_bytes(length)?;
        blocks.push(PlayerUpdateBlock377::Chat {
            colour: colour_effect >> 8,
            effect: colour_effect & 0xFF,
            rights,
            packed: TransformedBytes377 {
                wire,
                transform: DataTransform377::Subtract128,
            },
        });
    }
    if mask & MASK_DAMAGE2 != 0 {
        blocks.push(PlayerUpdateBlock377::Damage2 {
            amount: p.g1_alt3()?,
            kind: p.g1_alt2()?,
            current_hitpoints: p.g1_alt3()?,
            base_hitpoints: p.g1()?,
        });
    }
    Ok(())
}

fn optional_id(id: i32) -> Option<i32> {
    (id != 65535).then_some(id)
}

fn finish_payload(p: &PacketReader377<'_>) -> Result<(), PacketError377> {
    if p.remaining() == 0 {
        Ok(())
    } else {
        Err(p.error(PacketErrorKind377::TrailingBytes(p.remaining())))
    }
}

struct AppearanceCursor377<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> AppearanceCursor377<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn require(
        &self,
        p: &PacketReader377<'_>,
        frame_offset: usize,
        needed: usize,
    ) -> Result<(), PacketError377> {
        if self.remaining() >= needed {
            Ok(())
        } else {
            let mut error = p.error(PacketErrorKind377::Truncated {
                needed,
                remaining: self.remaining(),
            });
            error.offset = frame_offset + self.position;
            Err(error)
        }
    }
    fn g1(&mut self, p: &PacketReader377<'_>, frame_offset: usize) -> Result<i32, PacketError377> {
        self.require(p, frame_offset, 1)?;
        let wire = self.bytes[self.bytes.len() - self.position - 1];
        self.position += 1;
        Ok(wire as i32)
    }
    fn g1b(&mut self, p: &PacketReader377<'_>, frame_offset: usize) -> Result<i32, PacketError377> {
        Ok(self.g1(p, frame_offset)? as i8 as i32)
    }
    fn g2(&mut self, p: &PacketReader377<'_>, frame_offset: usize) -> Result<i32, PacketError377> {
        self.require(p, frame_offset, 2)?;
        let high = self.bytes[self.bytes.len() - self.position - 1] as i32;
        let low = self.bytes[self.bytes.len() - self.position - 2] as i32;
        self.position += 2;
        Ok((high << 8) | low)
    }
    fn g8(&mut self, p: &PacketReader377<'_>, frame_offset: usize) -> Result<i64, PacketError377> {
        self.require(p, frame_offset, 8)?;
        let mut bytes = [0; 8];
        for byte in &mut bytes {
            *byte = self.bytes[self.bytes.len() - self.position - 1];
            self.position += 1;
        }
        Ok(i64::from_be_bytes(bytes))
    }
}

/// Re-read the unchanged, previously validated reverse-order appearance cache.
pub(crate) fn decode_cached_appearance_377(
    wire: &[u8],
) -> Result<PlayerAppearance377<'_>, PacketError377> {
    let packet = PacketReader377::new(90, wire);
    parse_appearance(
        TransformedBytes377 {
            wire,
            transform: DataTransform377::Reverse,
        },
        &packet,
        0,
    )
}

fn parse_appearance<'a>(
    raw: TransformedBytes377<'a>,
    p: &PacketReader377<'_>,
    frame_offset: usize,
) -> Result<PlayerAppearance377<'a>, PacketError377> {
    let mut cursor = AppearanceCursor377::new(raw.wire);
    let gender = cursor.g1(p, frame_offset)?;
    let head_icon = cursor.g1b(p, frame_offset)?;
    let skull_icon = cursor.g1b(p, frame_offset)?;
    let mut equipment = [None; 12];
    let mut npc_transform = None;
    for (slot, part) in equipment.iter_mut().enumerate() {
        let high = cursor.g1(p, frame_offset)?;
        let value = if high == 0 {
            0
        } else {
            (high << 8) | cursor.g1(p, frame_offset)?
        };
        *part = Some(value);
        if slot == 0 && value == 65535 {
            npc_transform = Some(cursor.g2(p, frame_offset)?);
            break;
        }
    }
    let mut body_colours = [0; 5];
    for colour in &mut body_colours {
        *colour = cursor.g1(p, frame_offset)?;
    }
    let mut sequences = [None; 7];
    for sequence in &mut sequences {
        *sequence = optional_id(cursor.g2(p, frame_offset)?);
    }
    let username_base37 = cursor.g8(p, frame_offset)?;
    let combat_level = cursor.g1(p, frame_offset)?;
    let skill_level = cursor.g2(p, frame_offset)?;
    if cursor.remaining() != 0 {
        // Require exact method574 consumption instead of forwarding unknown tail data.
        let mut error = p.error(PacketErrorKind377::InvalidValue(
            "appearance_block_trailing_bytes",
        ));
        error.offset = frame_offset + cursor.position;
        return Err(error);
    }
    Ok(PlayerAppearance377 {
        raw,
        gender,
        head_icon,
        skull_icon,
        equipment,
        npc_transform,
        body_colours,
        sequences,
        username_base37,
        combat_level,
        skill_level,
    })
}

#[cfg(test)]
mod tests {
    use super::super::entity_377::test_support::{BitWriter377, TestContext377};
    use super::*;

    fn packet(bytes: &[u8]) -> PacketReader377<'_> {
        PacketReader377::new(90, bytes)
    }

    fn encode_g2(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&(value as u16).to_be_bytes());
    }
    fn encode_g2_alt2(out: &mut Vec<u8>, value: i32) {
        out.push((value >> 8) as u8);
        out.push((value as u8).wrapping_add(128));
    }
    fn encode_g1_alt1(out: &mut Vec<u8>, value: u8) {
        out.push(value.wrapping_add(128));
    }
    fn encode_g1_alt2(out: &mut Vec<u8>, value: u8) {
        out.push(value.wrapping_neg());
    }
    fn encode_g1_alt3(out: &mut Vec<u8>, value: u8) {
        out.push(128u8.wrapping_sub(value));
    }
    fn encode_g4_alt2(out: &mut Vec<u8>, value: i32) {
        let raw = value.to_be_bytes();
        out.extend_from_slice(&[raw[2], raw[3], raw[0], raw[1]]);
    }
    fn signed5(value: i32) -> i32 {
        value & 31
    }
    fn local_update_frame(mask: i32, body: &[u8]) -> Vec<u8> {
        let mut bits = BitWriter377::new();
        bits.write(1, 1);
        bits.write(2, 0);
        bits.write(8, 0);
        bits.write(11, PLAYER_NEW_INDEX_SENTINEL_377);
        bits.align();
        let mut bytes = bits.finish();
        if mask > 0xFF {
            bytes.push((mask as u8) | MASK_EXTENDED as u8);
            bytes.push((mask >> 8) as u8);
        } else {
            bytes.push(mask as u8);
        }
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn local_movement_decodes_idle_walk_run_and_teleport() {
        for operation in 0..4 {
            let mut bits = BitWriter377::new();
            bits.write(1, 1);
            bits.write(2, operation);
            match operation {
                0 => {}
                1 => {
                    bits.write(3, 6);
                    bits.write(1, 0);
                }
                2 => {
                    bits.write(3, 2);
                    bits.write(3, 7);
                    bits.write(1, 0);
                }
                3 => {
                    bits.write(1, 1);
                    bits.write(2, 3);
                    bits.write(7, 91);
                    bits.write(7, 37);
                    bits.write(1, 0);
                }
                _ => unreachable!(),
            }
            bits.write(8, 0);
            bits.write(11, PLAYER_NEW_INDEX_SENTINEL_377);
            bits.align();
            let mut bytes = bits.finish();
            if operation == 0 {
                bytes.push(0); // mask-only local update with no block bits
            }
            let mut reader = packet(&bytes);
            let decoded = PlayerInfo377::decode(&mut reader).unwrap();
            match operation {
                0 => assert_eq!(decoded.local_movement, None),
                1 => assert_eq!(
                    decoded.local_movement,
                    Some(ActorMovement377::Step {
                        first_direction: 6,
                        second_direction: None,
                    })
                ),
                2 => assert_eq!(
                    decoded.local_movement,
                    Some(ActorMovement377::Step {
                        first_direction: 2,
                        second_direction: Some(7),
                    })
                ),
                3 => assert_eq!(
                    decoded.local_movement,
                    Some(ActorMovement377::Teleport {
                        plane: Some(3),
                        x: 37,
                        z: 91,
                        jump: true,
                    })
                ),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn old_visible_idle_update_walk_run_and_removal_keep_ordinals() {
        let mut bits = BitWriter377::new();
        bits.write(1, 0);
        bits.write(8, 5);
        bits.write(1, 0); // ordinal 0 idle
        bits.write(1, 1);
        bits.write(2, 0); // ordinal 1 mask only
        bits.write(1, 1);
        bits.write(2, 1);
        bits.write(3, 4);
        bits.write(1, 0); // ordinal 2 walk
        bits.write(1, 1);
        bits.write(2, 2);
        bits.write(3, 1);
        bits.write(3, 7);
        bits.write(1, 1); // ordinal 3 run plus mask
        bits.write(1, 1);
        bits.write(2, 3); // ordinal 4 removal
        bits.write(11, PLAYER_NEW_INDEX_SENTINEL_377);
        bits.align();
        let mut bytes = bits.finish();
        bytes.push(0); // ordinal 1 has an empty mask
        bytes.push(0); // ordinal 3 has an empty mask

        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.old_visible_count, 5);
        assert_eq!(decoded.old_visible.len(), 5);
        assert!(!decoded.old_visible[0].removed);
        assert_eq!(decoded.old_visible[0].movement, None);
        assert_eq!(
            decoded.old_visible[2].movement,
            Some(ActorMovement377::Step {
                first_direction: 4,
                second_direction: None,
            })
        );
        assert_eq!(
            decoded.old_visible[3].movement,
            Some(ActorMovement377::Step {
                first_direction: 1,
                second_direction: Some(7),
            })
        );
        assert!(decoded.old_visible[4].removed);
        assert_eq!(decoded.updates[0].actor, ActorReference377::OldVisible(1));
        assert_eq!(decoded.updates[1].actor, ActorReference377::OldVisible(3));
        let mut context = TestContext377::default();
        context.visible_players = Some(5);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn new_player_offsets_are_signed_and_the_stop_sentinel_is_not_added() {
        let mut bits = BitWriter377::new();
        bits.write(1, 0);
        bits.write(8, 0);
        bits.write(11, 7);
        bits.write(5, signed5(-16));
        bits.write(1, 0);
        bits.write(1, 1);
        bits.write(5, signed5(15));
        bits.write(11, PLAYER_NEW_INDEX_SENTINEL_377);
        bits.align();
        let bytes = bits.finish();
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.additions.len(), 1);
        assert_eq!(decoded.additions[0].index, 7);
        assert_eq!(
            decoded.additions[0].movement,
            ActorMovement377::Offset {
                dx: -16,
                dz: 15,
                jump: true,
            }
        );
    }

    #[test]
    fn all_player_masks_extended_mask_and_nontrivial_transforms_decode() {
        let mut body = Vec::new();
        encode_g2(&mut body, 0x1234); // ANIM
        encode_g1_alt3(&mut body, 9);
        body.extend_from_slice(b"hey\n"); // SAY
        encode_g1_alt1(&mut body, 3); // EXACTMOVE start x
        encode_g1_alt2(&mut body, 4); // start z
        encode_g1_alt3(&mut body, 5); // end x
        body.push(6); // end z
        encode_g2(&mut body, 0x0102);
        encode_g2_alt2(&mut body, 0x0304);
        body.push(7); // direction
        encode_g2_alt2(&mut body, 5); // FACE_ENTITY: NPC slot 5
        encode_g2(&mut body, 0x1122);
        encode_g2(&mut body, 0x3344); // FACE_COORD
        encode_g2_alt2(&mut body, 2); // SPOT_ANIM
        encode_g4_alt2(&mut body, 0xFFFE_1234u32 as i32);

        let mut appearance = Vec::new();
        appearance.extend_from_slice(&[1, 0xFE, 0xFF]);
        appearance.extend_from_slice(&[0; 12]);
        appearance.extend_from_slice(&[0, 1, 2, 3, 4]);
        for sequence in [65535, 1, 65535, 2, 3, 4, 5] {
            encode_g2(&mut appearance, sequence);
        }
        appearance.extend_from_slice(&0x0102_0304_0506_0708i64.to_be_bytes());
        appearance.push(99);
        encode_g2(&mut appearance, 1200);
        body.push(appearance.len() as u8);
        body.extend(appearance.iter().rev().copied()); // gdata_alt1 reverses

        encode_g1_alt1(&mut body, 10); // DAMAGE
        encode_g1_alt3(&mut body, 11);
        encode_g1_alt2(&mut body, 12);
        body.push(13);
        encode_g2(&mut body, 0xA1B2); // CHAT colour/effect
        encode_g1_alt2(&mut body, 2);
        encode_g1_alt1(&mut body, 3);
        body.extend_from_slice(&[0x81, 0x00, 0x7F]); // decoded [1, 128, 255]
        encode_g1_alt3(&mut body, 14); // DAMAGE2
        encode_g1_alt2(&mut body, 15);
        encode_g1_alt3(&mut body, 16);
        body.push(17);

        let bytes = local_update_frame(MASK_KNOWN | MASK_EXTENDED, &body);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.updates.len(), 1);
        assert_eq!(decoded.updates[0].actor, ActorReference377::LocalPlayer);
        assert_eq!(decoded.blocks.len(), 10);
        assert_eq!(
            decoded.blocks[0],
            PlayerUpdateBlock377::Animation {
                sequence: Some(0x1234),
                delay: 9,
            }
        );
        assert_eq!(
            decoded.blocks[1],
            PlayerUpdateBlock377::Say { text: b"hey" }
        );
        assert_eq!(
            decoded.blocks[2],
            PlayerUpdateBlock377::ExactMove {
                start_x: 3,
                start_z: 4,
                end_x: 5,
                end_z: 6,
                start_delay: 0x0102,
                end_delay: 0x0304,
                direction: 7,
            }
        );
        assert_eq!(
            decoded.blocks[3],
            PlayerUpdateBlock377::FaceEntity(ActorTargetField377 {
                wire_value: 5,
                target: Some(super::super::entity_377::ActorTarget377::NpcSlot(5)),
            })
        );
        assert_eq!(
            decoded.blocks[4],
            PlayerUpdateBlock377::FaceCoordinate {
                x: 0x1122,
                z: 0x3344,
            }
        );
        assert_eq!(
            decoded.blocks[5],
            PlayerUpdateBlock377::SpotAnimation {
                animation: Some(2),
                height: -2,
                delay: 0x1234,
            }
        );
        let PlayerUpdateBlock377::Appearance(appearance) = &decoded.blocks[6] else {
            panic!("appearance block");
        };
        assert_eq!(appearance.gender, 1);
        assert_eq!(appearance.head_icon, -2);
        assert_eq!(appearance.skull_icon, -1);
        assert_eq!(appearance.equipment, [Some(0); 12]);
        assert_eq!(appearance.body_colours, [0, 1, 2, 3, 4]);
        assert_eq!(
            appearance.sequences,
            [None, Some(1), None, Some(2), Some(3), Some(4), Some(5)]
        );
        assert_eq!(appearance.username_base37, 0x0102_0304_0506_0708);
        assert_eq!(appearance.combat_level, 99);
        assert_eq!(appearance.skill_level, 1200);
        assert_eq!(
            appearance.raw.decoded_iter().collect::<Vec<_>>(),
            appearance_bytes_for_test()
        );
        assert_eq!(
            decoded.blocks[7],
            PlayerUpdateBlock377::Damage {
                amount: 10,
                kind: 11,
                current_hitpoints: 12,
                base_hitpoints: 13,
            }
        );
        let PlayerUpdateBlock377::Chat {
            colour,
            effect,
            rights,
            packed,
        } = &decoded.blocks[8]
        else {
            panic!("chat block");
        };
        assert_eq!((*colour, *effect, *rights), (0xA1, 0xB2, 2));
        assert_eq!(packed.decoded_iter().collect::<Vec<_>>(), [1, 128, 255]);
        assert_eq!(
            decoded.blocks[9],
            PlayerUpdateBlock377::Damage2 {
                amount: 14,
                kind: 15,
                current_hitpoints: 16,
                base_hitpoints: 17,
            }
        );

        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        for sequence in [1, 2, 3, 4, 5, 0x1234] {
            context.allow(ReferenceKind377::Sequence, sequence);
        }
        context.allow(ReferenceKind377::Spotanim, 2);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn absent_animation_and_spot_animation_sentinels_need_no_resources() {
        let mut body = Vec::new();
        encode_g2(&mut body, 65535); // ANIM absent
        encode_g1_alt3(&mut body, 0);
        encode_g2_alt2(&mut body, 65535); // SPOT_ANIM absent
        encode_g4_alt2(&mut body, 0);
        let bytes = local_update_frame(MASK_ANIMATION | MASK_SPOT_ANIMATION, &body);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.blocks.len(), 2);
        assert_eq!(
            decoded.blocks[0],
            PlayerUpdateBlock377::Animation {
                sequence: None,
                delay: 0,
            }
        );
        assert_eq!(
            decoded.blocks[1],
            PlayerUpdateBlock377::SpotAnimation {
                animation: None,
                height: 0,
                delay: 0,
            }
        );
        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        decoded.validate(&context).unwrap();
    }

    fn appearance_bytes_for_test() -> Vec<u8> {
        let mut bytes = vec![1, 0xFE, 0xFF];
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&[0, 1, 2, 3, 4]);
        for sequence in [65535, 1, 65535, 2, 3, 4, 5] {
            bytes.extend_from_slice(&(sequence as u16).to_be_bytes());
        }
        bytes.extend_from_slice(&0x0102_0304_0506_0708i64.to_be_bytes());
        bytes.push(99);
        bytes.extend_from_slice(&1200u16.to_be_bytes());
        bytes
    }

    #[test]
    fn appearance_block_truncation_and_unknown_tail_fail_closed() {
        let mut short = appearance_bytes_for_test();
        short.pop();
        let mut body = vec![short.len() as u8];
        body.extend(short.iter().rev().copied());
        let bytes = local_update_frame(MASK_APPEARANCE, &body);
        let mut reader = packet(&bytes);
        assert!(matches!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::Truncated { .. }
        ));

        let mut long = appearance_bytes_for_test();
        long.push(0);
        let mut body = vec![long.len() as u8];
        body.extend(long.iter().rev().copied());
        let bytes = local_update_frame(MASK_APPEARANCE, &body);
        let mut reader = packet(&bytes);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("appearance_block_trailing_bytes")
        );
    }

    #[test]
    fn appearance_transmog_branch_preserves_only_encoded_equipment_prefix() {
        let mut appearance = vec![2, 1, 0, 0xFF, 0xFF];
        encode_g2(&mut appearance, 77);
        appearance.extend_from_slice(&[1, 2, 3, 4, 5]);
        for sequence in [65535; 7] {
            encode_g2(&mut appearance, sequence);
        }
        appearance.extend_from_slice(&0i64.to_be_bytes());
        appearance.push(1);
        encode_g2(&mut appearance, 2);
        let mut body = vec![appearance.len() as u8];
        body.extend(appearance.iter().rev().copied());
        let bytes = local_update_frame(MASK_APPEARANCE, &body);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        let PlayerUpdateBlock377::Appearance(value) = &decoded.blocks[0] else {
            panic!("appearance block");
        };
        assert_eq!(value.equipment[0], Some(65535));
        assert!(value.equipment[1..].iter().all(Option::is_none));
        assert_eq!(value.npc_transform, Some(77));
        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Npc,
                id: 77,
            })
        );
        context.allow(ReferenceKind377::Npc, 77);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn equipment_identity_kit_and_object_dependencies_fail_closed() {
        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        for (equipment, kind) in [
            (256, ReferenceKind377::IdentityKit),
            (512, ReferenceKind377::Object),
        ] {
            let mut appearance = vec![0, 0, 0, (equipment >> 8) as u8, equipment as u8];
            appearance.extend_from_slice(&[0; 11]);
            appearance.extend_from_slice(&[0; 5]);
            for sequence in [65535; 7] {
                encode_g2(&mut appearance, sequence);
            }
            appearance.extend_from_slice(&0i64.to_be_bytes());
            appearance.push(1);
            encode_g2(&mut appearance, 2);
            let mut body = vec![appearance.len() as u8];
            body.extend(appearance.iter().rev().copied());
            let bytes = local_update_frame(MASK_APPEARANCE, &body);
            let mut reader = packet(&bytes);
            let decoded = PlayerInfo377::decode(&mut reader).unwrap();
            assert_eq!(
                decoded.validate(&context),
                Err(PacketErrorKind377::InvalidReference { kind, id: 0 })
            );
        }
    }

    #[test]
    fn unknown_extended_mask_bits_fail_before_block_decode() {
        let bytes = local_update_frame(0x820, &[]);
        let before = bytes.clone();
        let mut reader = packet(&bytes);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("unknown_player_update_mask")
        );
        assert_eq!(bytes, before);
    }

    #[test]
    fn frame_truncation_trailing_bytes_and_count_mismatch_are_rejected() {
        let truncated = [0u8];
        let mut reader = packet(&truncated);
        assert!(matches!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::Truncated { .. }
        ));

        let mut trailing = local_update_frame(0, &[]);
        trailing.push(0);
        let mut reader = packet(&trailing);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::TrailingBytes(1)
        );

        let mut bits = BitWriter377::new();
        bits.write(1, 0);
        bits.write(8, 2);
        bits.write(1, 0);
        bits.write(1, 0);
        bits.align();
        let bytes = bits.finish();
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        let mut context = TestContext377::default();
        context.visible_players = Some(1);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidValue("old_visible_player_count"))
        );
    }

    #[test]
    fn duplicate_player_indices_and_invalid_face_entity_encodings_fail() {
        let mut bits = BitWriter377::new();
        bits.write(1, 0);
        bits.write(8, 0);
        for _ in 0..2 {
            bits.write(11, 7);
            bits.write(5, 0);
            bits.write(1, 0);
            bits.write(1, 0);
            bits.write(5, 0);
        }
        bits.align();
        let bytes = bits.finish();
        let mut reader = packet(&bytes);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("duplicate_new_player_index")
        );

        // 0x4000 hits Java's NPC target branch but exceeds its slot array.
        let bytes = local_update_frame(MASK_FACE_ENTITY, &[0x40, 0x80]);
        let mut reader = packet(&bytes);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("face_entity_npc_slot")
        );
        // 0x8800 targets player slot 0x0800, beyond Java's 2048-slot array.
        let bytes = local_update_frame(MASK_FACE_ENTITY, &[0x88, 0x80]);
        let mut reader = packet(&bytes);
        assert_eq!(
            PlayerInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("face_entity_player_slot")
        );

        let mut body = Vec::new();
        encode_g2_alt2(&mut body, 32768 + 2047);
        let bytes = local_update_frame(MASK_FACE_ENTITY, &body);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(
            decoded.blocks[0],
            PlayerUpdateBlock377::FaceEntity(ActorTargetField377 {
                wire_value: 32768 + 2047,
                target: Some(super::super::entity_377::ActorTarget377::PlayerSlot(2047)),
            })
        );
        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn sequence_and_spotanim_references_fail_closed() {
        let mut anim = Vec::new();
        encode_g2(&mut anim, 7);
        encode_g1_alt3(&mut anim, 0);
        let bytes = local_update_frame(MASK_ANIMATION, &anim);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        let mut context = TestContext377::default();
        context.visible_players = Some(0);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Sequence,
                id: 7,
            })
        );

        let mut spot = Vec::new();
        encode_g2_alt2(&mut spot, 8);
        encode_g4_alt2(&mut spot, 0);
        let bytes = local_update_frame(MASK_SPOT_ANIMATION, &spot);
        let mut reader = packet(&bytes);
        let decoded = PlayerInfo377::decode(&mut reader).unwrap();
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Spotanim,
                id: 8,
            })
        );
    }
}
