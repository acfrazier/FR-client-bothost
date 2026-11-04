//! Revision-377 opcode 71 NPC-info decoder.
//!
//! Java anchors: `Client.java:9364-9577` and `Packet.java:310-416`. The decoder
//! stages old-visible movement, new NPC definitions, and every one-byte mask
//! block. It does not mutate actor state or resolve an NPC definition.
//!
//! Engine source cross-check: `src/network/rsbuf/info.ts:270-467` writes the NPC
//! bit/mask order, while `src/network/rsbuf/messages.ts:222-377` provides byte
//! encoders. `src/network/game/server/codec/NpcInfoEncoder.ts:9-11` only copies
//! completed bytes.
//! Truncation, duplicate additions, bad actor slots, unresolved definitions,
//! and trailing frame bytes fail closed before the staged operation is handed
//! off. No partial actor mutation is reproduced here.

use super::entity_377::{
    actor_target_field_matches, decode_actor_target, ActorMovement377, ActorReference377,
    ActorTargetField377, OldVisibleActor377, NPC_NEW_INDEX_SENTINEL_377,
};
use super::{
    require_reference, PacketContext377, PacketError377, PacketErrorKind377, PacketReader377,
    ReferenceKind377,
};
use std::ops::Range;

const MASK_CHANGE_TYPE: i32 = 0x01;
const MASK_FACE_ENTITY: i32 = 0x40;
const MASK_DAMAGE: i32 = 0x80;
const MASK_SPOT_ANIMATION: i32 = 0x04;
const MASK_SAY: i32 = 0x20;
const MASK_FACE_COORDINATE: i32 = 0x08;
const MASK_ANIMATION: i32 = 0x02;
const MASK_DAMAGE2: i32 = 0x10;
const MASK_KNOWN: i32 = 0xFF;

/// One decoded opcode-71 frame. Old-visible references use pre-frame ordinals.
#[derive(Debug, PartialEq, Eq)]
pub struct NpcInfo377<'a> {
    /// Number of old-visible NPCs described by this frame.
    ///
    /// Java removes old-list entries after this ordinal when the wire count is
    /// smaller than the actual pre-frame list.
    pub old_visible_count: usize,
    /// One entry for each wire old-visible ordinal, including explicit removals.
    pub old_visible: Vec<OldVisibleActor377>,
    /// Newly visible NPC slots, in packet order.
    pub additions: Vec<NpcAddition377>,
    /// Mask-bearing actors, in Java's old/new update order.
    pub updates: Vec<NpcActorUpdate377>,
    /// Flat mask-block storage. Each update's `blocks` range selects its blocks.
    pub blocks: Vec<NpcUpdateBlock377<'a>>,
}

/// A new NPC slot, definition and signed local-tile offset.
#[derive(Debug, PartialEq, Eq)]
pub struct NpcAddition377 {
    /// NPC actor-array slot. 16383 is the wire stop sentinel.
    pub index: i32,
    /// NPC definition selected by the 13-bit new-visible field.
    pub definition_id: i32,
    /// Signed offsets follow Java's five-bit two's-complement rule.
    pub movement: ActorMovement377,
}

/// The contiguous mask blocks for one NPC actor.
#[derive(Debug, PartialEq, Eq)]
pub struct NpcActorUpdate377 {
    pub actor: ActorReference377,
    pub blocks: Range<usize>,
}

/// One Java NPC update block, decoded in Java's mask order.
#[derive(Debug, PartialEq, Eq)]
pub enum NpcUpdateBlock377<'a> {
    ChangeType {
        definition_id: i32,
    },
    FaceEntity(ActorTargetField377),
    Damage {
        amount: i32,
        kind: i32,
        current_hitpoints: i32,
        base_hitpoints: i32,
    },
    SpotAnimation {
        animation: Option<i32>,
        height: i32,
        delay: i32,
    },
    Say {
        text: &'a [u8],
    },
    FaceCoordinate {
        x: i32,
        z: i32,
    },
    Animation {
        sequence: Option<i32>,
        delay: i32,
    },
    Damage2 {
        amount: i32,
        kind: i32,
        current_hitpoints: i32,
        base_hitpoints: i32,
    },
}

impl<'a> NpcInfo377<'a> {
    /// Decode one complete opcode-71 payload without changing client state.
    pub(super) fn decode(p: &mut PacketReader377<'a>) -> Result<Self, PacketError377> {
        let mut bits = p.bits();
        let total_bits = bits.bit_position() + bits.remaining_bits();
        let old_visible_count = bits.gbit(8)? as usize;
        let mut old_visible = Vec::with_capacity(old_visible_count);
        let mut updates = Vec::new();

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
                    updates.push(NpcActorUpdate377 {
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
                        updates.push(NpcActorUpdate377 {
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
                        updates.push(NpcActorUpdate377 {
                            actor: ActorReference377::OldVisible(ordinal),
                            blocks: 0..0,
                        });
                    }
                }
                3 => old_visible.push(OldVisibleActor377 {
                    movement: None,
                    removed: true,
                }),
                _ => unreachable!("two-bit old-NPC movement value"),
            }
        }

        // Preserve Java's exact guard: bitPos + 21 < psize * 8.
        let mut additions = Vec::new();
        let mut seen_additions = [0u64; 256];
        while bits.bit_position() + 21 < total_bits {
            let index = bits.gbit(14)?;
            if index == NPC_NEW_INDEX_SENTINEL_377 {
                break;
            }
            let slot = index as usize;
            let word = slot / 64;
            let bit = 1u64 << (slot % 64);
            if seen_additions[word] & bit != 0 {
                return Err(p.invalid("duplicate_new_npc_index"));
            }
            seen_additions[word] |= bit;
            let update = bits.gbit(1)? != 0;
            let dz = signed_five(bits.gbit(5)?);
            let dx = signed_five(bits.gbit(5)?);
            let jump = bits.gbit(1)? != 0;
            let definition_id = bits.gbit(13)?;
            additions.push(NpcAddition377 {
                index,
                definition_id,
                movement: ActorMovement377::Offset { dx, dz, jump },
            });
            if update {
                updates.push(NpcActorUpdate377 {
                    actor: ActorReference377::New(index),
                    blocks: 0..0,
                });
            }
        }
        bits.access_bytes();

        let mut blocks = Vec::new();
        for update in &mut updates {
            let start = blocks.len();
            let mask = p.g1()?;
            if mask & !MASK_KNOWN != 0 {
                return Err(p.invalid("unknown_npc_update_mask"));
            }
            decode_npc_blocks(mask, p, &mut blocks)?;
            update.blocks = start..blocks.len();
        }
        finish_payload(p)?;

        Ok(Self {
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
            .visible_npc_count()
            .ok_or(PacketErrorKind377::InvalidValue("visible_npc_count"))?;
        if self.old_visible_count != self.old_visible.len()
            || self.old_visible_count > visible_count
        {
            return Err(PacketErrorKind377::InvalidValue("old_visible_npc_count"));
        }

        let mut additions = [0u64; 256];
        for addition in &self.additions {
            if addition.index < 0 || addition.index >= NPC_NEW_INDEX_SENTINEL_377 {
                return Err(PacketErrorKind377::InvalidValue("new_npc_index"));
            }
            let slot = addition.index as usize;
            let word = slot / 64;
            let bit = 1u64 << (slot % 64);
            if additions[word] & bit != 0 {
                return Err(PacketErrorKind377::InvalidValue("duplicate_new_npc_index"));
            }
            additions[word] |= bit;
            require_reference(context, R::Npc, addition.definition_id)?;
        }

        let mut old_updates = [0u64; 4];
        let mut new_updates = [0u64; 256];
        let mut next_block = 0;
        for update in &self.updates {
            if update.blocks.start != next_block
                || update.blocks.start > update.blocks.end
                || update.blocks.end > self.blocks.len()
            {
                return Err(PacketErrorKind377::InvalidValue("npc_update_block_range"));
            }
            next_block = update.blocks.end;
            match update.actor {
                ActorReference377::OldVisible(ordinal)
                    if ordinal < self.old_visible_count
                        && !self.old_visible[ordinal].removed
                        && old_updates[ordinal / 64] & (1u64 << (ordinal % 64)) == 0 =>
                {
                    old_updates[ordinal / 64] |= 1u64 << (ordinal % 64);
                }
                ActorReference377::OldVisible(_) => {
                    return Err(PacketErrorKind377::InvalidValue("npc_update_old_ordinal"));
                }
                ActorReference377::New(index)
                    if index >= 0
                        && (index as usize) < NPC_NEW_INDEX_SENTINEL_377 as usize
                        && additions[index as usize / 64] & (1u64 << (index as usize % 64))
                            != 0
                        && new_updates[index as usize / 64] & (1u64 << (index as usize % 64))
                            == 0 =>
                {
                    new_updates[index as usize / 64] |= 1u64 << (index as usize % 64);
                }
                ActorReference377::New(_) => {
                    return Err(PacketErrorKind377::InvalidValue("npc_update_new_index"));
                }
                ActorReference377::LocalPlayer => {
                    return Err(PacketErrorKind377::InvalidValue("npc_local_player_update"));
                }
            }
            for block in &self.blocks[update.blocks.clone()] {
                match block {
                    NpcUpdateBlock377::ChangeType { definition_id } => {
                        require_reference(context, R::Npc, *definition_id)?;
                    }
                    NpcUpdateBlock377::Animation {
                        sequence: Some(sequence),
                        ..
                    } => require_reference(context, R::Sequence, *sequence)?,
                    NpcUpdateBlock377::SpotAnimation {
                        animation: Some(animation),
                        ..
                    } => require_reference(context, R::Spotanim, *animation)?,
                    NpcUpdateBlock377::FaceEntity(target)
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
                "unassigned_npc_update_blocks",
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

fn decode_npc_blocks<'a>(
    mask: i32,
    p: &mut PacketReader377<'a>,
    blocks: &mut Vec<NpcUpdateBlock377<'a>>,
) -> Result<(), PacketError377> {
    if mask & MASK_CHANGE_TYPE != 0 {
        blocks.push(NpcUpdateBlock377::ChangeType {
            definition_id: p.g2_alt2()?,
        });
    }
    if mask & MASK_FACE_ENTITY != 0 {
        let wire_value = p.g2_alt1()?;
        blocks.push(NpcUpdateBlock377::FaceEntity(decode_actor_target(
            wire_value, p,
        )?));
    }
    if mask & MASK_DAMAGE != 0 {
        blocks.push(NpcUpdateBlock377::Damage {
            amount: p.g1_alt1()?,
            kind: p.g1_alt1()?,
            current_hitpoints: p.g1()?,
            base_hitpoints: p.g1_alt3()?,
        });
    }
    if mask & MASK_SPOT_ANIMATION != 0 {
        let animation = optional_id(p.g2()?);
        let packed = p.g4_alt2()?;
        blocks.push(NpcUpdateBlock377::SpotAnimation {
            animation,
            height: packed >> 16,
            delay: packed & 0xFFFF,
        });
    }
    if mask & MASK_SAY != 0 {
        blocks.push(NpcUpdateBlock377::Say { text: p.gjstr()? });
    }
    if mask & MASK_FACE_COORDINATE != 0 {
        blocks.push(NpcUpdateBlock377::FaceCoordinate {
            x: p.g2_alt3()?,
            z: p.g2_alt1()?,
        });
    }
    if mask & MASK_ANIMATION != 0 {
        blocks.push(NpcUpdateBlock377::Animation {
            sequence: optional_id(p.g2()?),
            delay: p.g1_alt3()?,
        });
    }
    if mask & MASK_DAMAGE2 != 0 {
        blocks.push(NpcUpdateBlock377::Damage2 {
            amount: p.g1_alt3()?,
            kind: p.g1_alt3()?,
            current_hitpoints: p.g1()?,
            base_hitpoints: p.g1_alt2()?,
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

#[cfg(test)]
mod tests {
    use super::super::entity_377::test_support::{BitWriter377, TestContext377};
    use super::*;

    fn packet(bytes: &[u8]) -> PacketReader377<'_> {
        PacketReader377::new(71, bytes)
    }
    fn encode_g2(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&(value as u16).to_be_bytes());
    }
    fn encode_g2_alt2(out: &mut Vec<u8>, value: i32) {
        out.push((value >> 8) as u8);
        out.push((value as u8).wrapping_add(128));
    }
    fn encode_g2_alt1(out: &mut Vec<u8>, value: i32) {
        out.push(value as u8);
        out.push((value >> 8) as u8);
    }
    fn encode_g2_alt3(out: &mut Vec<u8>, value: i32) {
        out.push((value as u8).wrapping_add(128));
        out.push((value >> 8) as u8);
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
    fn old_update_frame(mask: u8, body: &[u8]) -> Vec<u8> {
        let mut bits = BitWriter377::new();
        bits.write(8, 1);
        bits.write(1, 1);
        bits.write(2, 0);
        bits.write(14, NPC_NEW_INDEX_SENTINEL_377);
        bits.align();
        let mut bytes = bits.finish();
        bytes.push(mask);
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn old_visible_idle_update_walk_run_and_removal_keep_ordinals() {
        let mut bits = BitWriter377::new();
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
        bits.write(14, NPC_NEW_INDEX_SENTINEL_377);
        bits.align();
        let mut bytes = bits.finish();
        bytes.push(0); // ordinal 1 has an empty mask
        bytes.push(0); // ordinal 3 has an empty mask

        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.old_visible_count, 5);
        assert_eq!(decoded.old_visible.len(), 5);
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
        context.visible_npcs = Some(5);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn new_npc_signed_offsets_definition_and_stop_sentinel_decode() {
        let mut bits = BitWriter377::new();
        bits.write(8, 0);
        bits.write(14, 10);
        bits.write(1, 1);
        bits.write(5, signed5(-16)); // dz
        bits.write(5, signed5(15)); // dx
        bits.write(1, 1);
        bits.write(13, 321);
        bits.write(14, NPC_NEW_INDEX_SENTINEL_377);
        bits.align();
        let mut bytes = bits.finish();
        bytes.push(0); // empty extended block keeps Java's sentinel guard in range
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.additions.len(), 1);
        assert_eq!(decoded.updates[0].actor, ActorReference377::New(10));
        assert!(decoded.blocks.is_empty());
        assert_eq!(decoded.additions[0].index, 10);
        assert_eq!(decoded.additions[0].definition_id, 321);
        assert_eq!(
            decoded.additions[0].movement,
            ActorMovement377::Offset {
                dx: 15,
                dz: -16,
                jump: true,
            }
        );
        let mut context = TestContext377::default();
        context.visible_npcs = Some(0);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Npc,
                id: 321,
            })
        );
        context.allow(ReferenceKind377::Npc, 321);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn all_npc_masks_and_nontrivial_transforms_decode() {
        let mut body = Vec::new();
        encode_g2_alt2(&mut body, 77); // CHANGE_TYPE
        encode_g2_alt1(&mut body, 6); // FACE_ENTITY NPC slot 6
        encode_g1_alt1(&mut body, 10); // DAMAGE
        encode_g1_alt1(&mut body, 11);
        body.push(12);
        encode_g1_alt3(&mut body, 13);
        encode_g2(&mut body, 4); // SPOT_ANIM
        encode_g4_alt2(&mut body, 0xFFFD_4567u32 as i32);
        body.extend_from_slice(b"yo\n"); // SAY
        encode_g2_alt3(&mut body, 0x1122); // FACE_COORD
        encode_g2_alt1(&mut body, 0x3344);
        encode_g2(&mut body, 7); // ANIM
        encode_g1_alt3(&mut body, 8);
        encode_g1_alt3(&mut body, 14); // DAMAGE2
        encode_g1_alt3(&mut body, 15);
        body.push(16);
        encode_g1_alt2(&mut body, 17);

        let bytes = old_update_frame(MASK_KNOWN as u8, &body);
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.blocks.len(), 8);
        assert_eq!(
            decoded.blocks[0],
            NpcUpdateBlock377::ChangeType { definition_id: 77 }
        );
        assert_eq!(
            decoded.blocks[1],
            NpcUpdateBlock377::FaceEntity(ActorTargetField377 {
                wire_value: 6,
                target: Some(super::super::entity_377::ActorTarget377::NpcSlot(6)),
            })
        );
        assert_eq!(
            decoded.blocks[2],
            NpcUpdateBlock377::Damage {
                amount: 10,
                kind: 11,
                current_hitpoints: 12,
                base_hitpoints: 13,
            }
        );
        assert_eq!(
            decoded.blocks[3],
            NpcUpdateBlock377::SpotAnimation {
                animation: Some(4),
                height: -3,
                delay: 0x4567,
            }
        );
        assert_eq!(decoded.blocks[4], NpcUpdateBlock377::Say { text: b"yo" });
        assert_eq!(
            decoded.blocks[5],
            NpcUpdateBlock377::FaceCoordinate {
                x: 0x1122,
                z: 0x3344
            }
        );
        assert_eq!(
            decoded.blocks[6],
            NpcUpdateBlock377::Animation {
                sequence: Some(7),
                delay: 8
            }
        );
        assert_eq!(
            decoded.blocks[7],
            NpcUpdateBlock377::Damage2 {
                amount: 14,
                kind: 15,
                current_hitpoints: 16,
                base_hitpoints: 17,
            }
        );

        let mut context = TestContext377::default();
        context.visible_npcs = Some(1);
        context.allow(ReferenceKind377::Npc, 77);
        context.allow(ReferenceKind377::Sequence, 7);
        context.allow(ReferenceKind377::Spotanim, 4);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn face_entity_sentinel_and_array_bounds_follow_java_encoding() {
        let bytes = old_update_frame(0x40, &[0xFF, 0xFF]);
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(
            decoded.blocks[0],
            NpcUpdateBlock377::FaceEntity(ActorTargetField377 {
                wire_value: 65535,
                target: None,
            })
        );

        let bytes = old_update_frame(0x40, &[0x00, 0x40]); // little-endian 0x4000.
        let mut reader = packet(&bytes);
        assert_eq!(
            NpcInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("face_entity_npc_slot")
        );
        let bytes = old_update_frame(0x40, &[0x00, 0x88]); // player slot 0x0800.
        let mut reader = packet(&bytes);
        assert_eq!(
            NpcInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("face_entity_player_slot")
        );
    }

    #[test]
    fn absent_sequence_and_spot_animation_sentinels_need_no_resources() {
        let mut body = Vec::new();
        encode_g2(&mut body, 65535); // SPOT_ANIM absent
        encode_g4_alt2(&mut body, 0);
        encode_g2(&mut body, 65535); // ANIM absent
        encode_g1_alt3(&mut body, 0);
        let bytes = old_update_frame(0x06, &body);
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(decoded.blocks.len(), 2);
        assert_eq!(
            decoded.blocks[0],
            NpcUpdateBlock377::SpotAnimation {
                animation: None,
                height: 0,
                delay: 0,
            }
        );
        assert_eq!(
            decoded.blocks[1],
            NpcUpdateBlock377::Animation {
                sequence: None,
                delay: 0,
            }
        );
        let mut context = TestContext377::default();
        context.visible_npcs = Some(1);
        decoded.validate(&context).unwrap();
    }

    #[test]
    fn truncation_trailing_bytes_count_mismatch_and_resources_fail_closed() {
        let truncated = [1u8];
        let mut reader = packet(&truncated);
        assert!(matches!(
            NpcInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::Truncated { .. }
        ));

        let mut trailing = vec![0];
        trailing.push(0);
        let mut reader = packet(&trailing);
        assert_eq!(
            NpcInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::TrailingBytes(1)
        );

        let mut bits = BitWriter377::new();
        bits.write(8, 1);
        bits.write(1, 0);
        bits.align();
        let bytes = bits.finish();
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        let mut context = TestContext377::default();
        context.visible_npcs = Some(0);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidValue("old_visible_npc_count"))
        );

        let bytes = old_update_frame(0x02, &[0, 7, 128]); // sequence 7, delay 0
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        context.visible_npcs = Some(1);
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Sequence,
                id: 7,
            })
        );

        let mut body = Vec::new();
        encode_g2_alt2(&mut body, 77);
        let bytes = old_update_frame(0x01, &body);
        let mut reader = packet(&bytes);
        let decoded = NpcInfo377::decode(&mut reader).unwrap();
        assert_eq!(
            decoded.validate(&context),
            Err(PacketErrorKind377::InvalidReference {
                kind: ReferenceKind377::Npc,
                id: 77,
            })
        );
    }

    #[test]
    fn duplicate_new_npc_indices_fail_before_handoff() {
        let mut bits = BitWriter377::new();
        bits.write(8, 0);
        for _ in 0..2 {
            bits.write(14, 10);
            bits.write(1, 0);
            bits.write(5, 0);
            bits.write(5, 0);
            bits.write(1, 0);
            bits.write(13, 321);
        }
        bits.align();
        let bytes = bits.finish();
        let before = bytes.clone();
        let mut reader = packet(&bytes);
        assert_eq!(
            NpcInfo377::decode(&mut reader).unwrap_err().kind,
            PacketErrorKind377::InvalidValue("duplicate_new_npc_index")
        );
        assert_eq!(bytes, before);
    }
}
