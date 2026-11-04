//! Semantic projections bind every decoded field to the independent wire corpus.
//! Arrays have an operation discriminator followed by named Rust fields in their
//! declaration order. No packet reader or inverse byte transform is used here.
use crate::packet_377::entity_377::{
    ActorMovement377 as M, ActorReference377 as A, ActorTarget377 as T, ActorTargetField377,
};
use crate::packet_377::npc_info_377::{NpcInfo377, NpcUpdateBlock377 as N};
use crate::packet_377::player_info_377::{
    PlayerAppearance377, PlayerInfo377, PlayerUpdateBlock377 as P,
};
use crate::packet_377::{
    interface_packet_377::InterfaceOperation377 as I, zone_377::ZoneOperation377 as Z,
    CameraTarget377, HintArrow377 as H, Operation377 as O,
};
use serde_json::{json, Value};

fn text(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn camera(kind: &str, v: &CameraTarget377) -> Value {
    json!([kind, v.x, v.z, v.height, v.rate, v.acceleration])
}
pub fn zone(op: &Z) -> Value {
    match op {
        Z::LocMerge {
            tile,
            shape,
            angle,
            location,
            min_x_offset,
            max_x_offset,
            min_z_offset,
            max_z_offset,
            player_slot,
            start_cycle,
            end_cycle,
        } => json!([
            "LocMerge",
            tile.x,
            tile.z,
            shape,
            angle,
            location,
            min_x_offset,
            max_x_offset,
            min_z_offset,
            max_z_offset,
            player_slot,
            start_cycle,
            end_cycle
        ]),
        Z::ObjectReveal {
            tile,
            count,
            object,
            receiver_id,
        } => json!(["ObjectReveal", tile.x, tile.z, count, object, receiver_id]),
        Z::LocAnim {
            tile,
            shape,
            angle,
            sequence,
        } => json!(["LocAnim", tile.x, tile.z, shape, angle, sequence]),
        Z::ObjAdd {
            tile,
            object,
            count,
        } => json!(["ObjAdd", tile.x, tile.z, object, count]),
        Z::ObjCount {
            tile,
            object,
            old_count,
            new_count,
        } => json!(["ObjCount", tile.x, tile.z, object, old_count, new_count]),
        Z::MapProjAnim {
            tile,
            destination_x_offset,
            destination_z_offset,
            target,
            spotanim,
            source_height,
            destination_height,
            start_delay,
            end_delay,
            peak,
            arc,
        } => json!([
            "MapProjAnim",
            tile.x,
            tile.z,
            destination_x_offset,
            destination_z_offset,
            target,
            spotanim,
            source_height,
            destination_height,
            start_delay,
            end_delay,
            peak,
            arc
        ]),
        Z::SoundArea {
            tile,
            sound,
            radius,
            loops,
        } => json!(["SoundArea", tile.x, tile.z, sound, radius, loops]),
        Z::MapAnim {
            tile,
            spotanim,
            height,
            delay,
        } => json!(["MapAnim", tile.x, tile.z, spotanim, height, delay]),
        Z::LocAddChange {
            tile,
            shape,
            angle,
            location,
        } => json!(["LocAddChange", tile.x, tile.z, shape, angle, location]),
        Z::ObjDel { tile, object } => json!(["ObjDel", tile.x, tile.z, object]),
        Z::LocDel { tile, shape, angle } => json!(["LocDel", tile.x, tile.z, shape, angle]),
    }
}
pub fn operation(op: &O<'_>) -> Value {
    match op {
        O::Interface(op) => match op {
            I::SetPosition { component, x, y } => json!(["SetPosition", component, x, y]),
            I::SetAngle {
                component,
                xan,
                yan,
                zoom,
            } => json!(["SetAngle", component, xan, yan, zoom]),
            I::SetModel { component, model } => json!(["SetModel", component, model]),
            I::SetNpcHead { component, npc } => json!(["SetNpcHead", component, npc]),
            I::OpenChat { component } => json!(["OpenChat", component]),
            I::TutOpen { component } => json!(["TutOpen", component]),
            I::SetColour { component, rgb555 } => json!(["SetColour", component, rgb555]),
            I::NameDialog => json!(["NameDialog"]),
            I::Close => json!(["Close"]),
            I::OpenOverlay { component } => json!(["OpenOverlay", component]),
            I::SetHide { component, hidden } => json!(["SetHide", component, hidden]),
            I::OpenMainSide { main, side } => json!(["OpenMainSide", main, side]),
            I::CountDialog => json!(["CountDialog"]),
            I::SetTabActive { tab } => json!(["SetTabActive", tab]),
            I::SetPlayerHead { component } => json!(["SetPlayerHead", component]),
            I::OpenMain { component } => json!(["OpenMain", component]),
            I::OpenSide { component } => json!(["OpenSide", component]),
            I::SetObject {
                component,
                object,
                divisor,
            } => json!(["SetObject", component, object, divisor]),
            I::SetAnim {
                component,
                sequence,
            } => json!(["SetAnim", component, sequence]),
            I::SetTab { tab, component } => json!(["SetTab", tab, component]),
            I::TutFlash { tab } => json!(["TutFlash", tab]),
            I::OpenFull { overlay, main } => json!(["OpenFull", overlay, main]),
            I::SetRotation { component, packed } => json!(["SetRotation", component, packed]),
            I::SetText {
                component,
                text: value,
            } => json!(["SetText", component, text(value)]),
            I::SetScrollPos {
                component,
                position,
            } => json!(["SetScrollPos", component, position]),
        },
        O::Zone(frame) => json!([
            "Zone",
            frame.origin(),
            frame.clear(),
            frame.operations().iter().map(zone).collect::<Vec<_>>()
        ]),
        O::PlayerInfo(info) => player(info),
        O::NpcInfo(info) => npc(info),
        O::Rebuild(crate::packet_377::rebuild_377::Rebuild377::Normal { zone_x, zone_z }) => {
            json!(["RebuildNormal", zone_x, zone_z])
        }
        O::Rebuild(crate::packet_377::rebuild_377::Rebuild377::Instance {
            zone_x,
            zone_z,
            templates,
        }) => json!(["RebuildInstance", zone_x, zone_z, templates]),
        O::InventoryFull { component, entries } => json!([
            "InventoryFull",
            component,
            entries
                .iter()
                .map(|e| [e.object_plus_one, e.count])
                .collect::<Vec<_>>()
        ]),
        O::InventoryPartial { component, entries } => json!([
            "InventoryPartial",
            component,
            entries
                .iter()
                .map(|e| [e.slot, e.entry.object_plus_one, e.entry.count])
                .collect::<Vec<_>>()
        ]),
        O::InventoryStopTransmit { component } => json!(["InventoryStopTransmit", component]),
        O::Varp { id, value } => json!(["Varp", id, value]),
        O::VarpSync => json!(["VarpSync"]),
        O::UpdateStat {
            stat,
            level,
            experience,
        } => json!(["UpdateStat", stat, level, experience]),
        O::RunEnergy(value) => json!(["RunEnergy", value]),
        O::RunWeight(value) => json!(["RunWeight", value]),
        O::ResetAnims => json!(["ResetAnims"]),
        O::UpdatePid { members, slot } => json!(["UpdatePid", members, slot]),
        O::Logout => json!(["Logout"]),
        O::LastLogin(v) => json!([
            "LastLogin",
            v.days_since_password_change,
            v.field371,
            v.unused_word,
            v.field504,
            v.current_day,
            v.unread_messages,
            v.previous_login_day,
            v.members_days,
            v.last_address,
            v.recovery_change_day,
            v.unused_byte
        ]),
        O::MessageGame(value) => json!(["MessageGame", text(value)]),
        O::PrivateMessage {
            from,
            message_id,
            staff_level,
            packed_body,
        } => json!([
            "PrivateMessage",
            from.to_string(),
            message_id,
            staff_level,
            hex(packed_body)
        ]),
        O::IgnoreList(names) => json!([
            "IgnoreList",
            names.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        ]),
        O::Friend { name, world } => json!(["Friend", name.to_string(), world]),
        O::FriendListLoaded(value) => json!(["FriendListLoaded", value]),
        O::ChatFilter {
            public,
            private,
            trade,
        } => json!(["ChatFilter", public, private, trade]),
        O::PlayerOption {
            index,
            text: value,
            priority,
        } => json!(["PlayerOption", index, text(value), priority]),
        O::SynthSound {
            sound,
            loops,
            delay,
        } => json!(["SynthSound", sound, loops, delay]),
        O::MidiSong(value) => json!(["MidiSong", value]),
        O::MidiJingle { song, delay } => json!(["MidiJingle", song, delay]),
        O::MinimapToggle(value) => json!(["MinimapToggle", value]),
        O::Multiway(value) => json!(["Multiway", value]),
        O::UnsetMapFlag => json!(["UnsetMapFlag"]),
        O::RebootTimer { ticks } => json!(["RebootTimer", ticks]),
        O::Hint(hint) => match hint {
            H::Npc { slot, padding } => json!(["HintNpc", slot, hex(padding)]),
            H::Player { slot, padding } => json!(["HintPlayer", slot, hex(padding)]),
            H::Tile { kind, x, z, height } => json!(["HintTile", kind, x, z, height]),
            H::Other { kind, padding } => json!(["HintOther", kind, hex(padding)]),
        },
        O::CameraLookAt(v) => camera("CameraLookAt", v),
        O::CameraMoveTo(v) => camera("CameraMoveTo", v),
        O::CameraShake {
            axis,
            jitter,
            amplitude,
            rate,
        } => json!(["CameraShake", axis, jitter, amplitude, rate]),
        O::CameraReset => json!(["CameraReset"]),
    }
}

fn actor(value: A) -> Value {
    match value {
        A::LocalPlayer => json!(["Local"]),
        A::OldVisible(ordinal) => json!(["Old", ordinal]),
        A::New(index) => json!(["New", index]),
    }
}
fn movement(value: Option<&M>, removed: bool, extended: bool) -> Value {
    if removed {
        assert!(value.is_none() && !extended);
        return json!(["Remove"]);
    }
    match value {
        None => json!([if extended { "Extended" } else { "Idle" }]),
        Some(M::Step {
            first_direction,
            second_direction: None,
        }) => json!(["Walk", first_direction, extended]),
        Some(M::Step {
            first_direction,
            second_direction: Some(second),
        }) => json!(["Run", first_direction, second, extended]),
        Some(M::Teleport { plane, x, z, jump }) => json!(["Teleport", jump, plane, x, z, extended]),
        Some(M::Offset { .. }) => panic!("offset in non-addition movement"),
    }
}
fn face(value: &ActorTargetField377) -> Value {
    let target = match value.target {
        None => 65535,
        Some(T::NpcSlot(index)) => index as i32,
        Some(T::PlayerSlot(index)) => index as i32 + 32768,
    };
    assert_eq!(target, value.wire_value, "face target interpretation");
    if target == 65535 {
        Value::Null
    } else {
        json!(target)
    }
}
fn appearance(value: &PlayerAppearance377<'_>) -> Value {
    let data = value.raw.decoded_iter().collect::<Vec<_>>();
    json!([
        hex(&data),
        value.gender,
        value.head_icon,
        value.skull_icon,
        value.equipment,
        value.npc_transform,
        value.body_colours,
        value.sequences,
        value.username_base37.to_string(),
        value.combat_level,
        value.skill_level
    ])
}
fn player(value: &PlayerInfo377<'_>) -> Value {
    let extended = |target| value.updates.iter().any(|update| update.actor == target);
    let old = value
        .old_visible
        .iter()
        .enumerate()
        .map(|(ordinal, old)| {
            movement(
                old.movement.as_ref(),
                old.removed,
                extended(A::OldVisible(ordinal)),
            )
        })
        .collect::<Vec<_>>();
    let additions = value
        .additions
        .iter()
        .map(|added| match added.movement {
            M::Offset { dx, dz, jump } => {
                json!([added.index, dx, dz, jump, extended(A::New(added.index))])
            }
            _ => panic!("non-offset player addition"),
        })
        .collect::<Vec<_>>();
    let updates = value
        .updates
        .iter()
        .map(|update| {
            let mut fields: [Value; 12] = std::array::from_fn(|_| Value::Null);
            fields[0] = actor(update.actor);
            let mut mask = 0;
            for block in &value.blocks[update.blocks.clone()] {
                let (index, bit, field) = match block {
                    P::Animation { sequence, delay } => (2, 8, json!([sequence, delay])),
                    P::Say { text: bytes } => (3, 16, json!(text(bytes))),
                    P::ExactMove {
                        start_x,
                        start_z,
                        end_x,
                        end_z,
                        start_delay,
                        end_delay,
                        direction,
                    } => (
                        4,
                        256,
                        json!([
                            start_x,
                            start_z,
                            end_x,
                            end_z,
                            start_delay,
                            end_delay,
                            direction
                        ]),
                    ),
                    P::FaceEntity(target) => (5, 1, face(target)),
                    P::FaceCoordinate { x, z } => (6, 2, json!([x, z])),
                    P::SpotAnimation {
                        animation,
                        height,
                        delay,
                    } => (7, 512, json!([animation, height, delay])),
                    P::Appearance(v) => (8, 4, appearance(v)),
                    P::Damage {
                        amount,
                        kind,
                        current_hitpoints,
                        base_hitpoints,
                    } => (
                        9,
                        1024,
                        json!([amount, kind, current_hitpoints, base_hitpoints]),
                    ),
                    P::Chat {
                        colour,
                        effect,
                        rights,
                        packed,
                    } => (
                        10,
                        64,
                        json!([
                            (colour << 8) | effect,
                            rights,
                            hex(&packed.decoded_iter().collect::<Vec<_>>())
                        ]),
                    ),
                    P::Damage2 {
                        amount,
                        kind,
                        current_hitpoints,
                        base_hitpoints,
                    } => (
                        11,
                        128,
                        json!([amount, kind, current_hitpoints, base_hitpoints]),
                    ),
                };
                assert_eq!(mask & bit, 0, "duplicate decoded player block");
                mask |= bit;
                fields[index] = field;
            }
            fields[1] = json!(if mask > 255 { mask | 32 } else { mask });
            json!(fields)
        })
        .collect::<Vec<_>>();
    json!([
        "PlayerInfo",
        movement(
            value.local_movement.as_ref(),
            false,
            extended(A::LocalPlayer)
        ),
        value.old_visible_count,
        old,
        additions,
        updates
    ])
}

fn npc(value: &NpcInfo377<'_>) -> Value {
    let extended = |target| value.updates.iter().any(|update| update.actor == target);
    let old = value
        .old_visible
        .iter()
        .enumerate()
        .map(|(ordinal, old)| {
            movement(
                old.movement.as_ref(),
                old.removed,
                extended(A::OldVisible(ordinal)),
            )
        })
        .collect::<Vec<_>>();
    let additions = value
        .additions
        .iter()
        .map(|added| match added.movement {
            M::Offset { dx, dz, jump } => json!([
                added.index,
                added.definition_id,
                dx,
                dz,
                jump,
                extended(A::New(added.index))
            ]),
            _ => panic!("non-offset NPC addition"),
        })
        .collect::<Vec<_>>();
    let updates = value
        .updates
        .iter()
        .map(|update| {
            let mut fields: [Value; 10] = std::array::from_fn(|_| Value::Null);
            fields[0] = actor(update.actor);
            let mut mask = 0;
            for block in &value.blocks[update.blocks.clone()] {
                let (index, bit, field) = match block {
                    N::ChangeType { definition_id } => (2, 1, json!(definition_id)),
                    N::FaceEntity(target) => (3, 64, face(target)),
                    N::Damage {
                        amount,
                        kind,
                        current_hitpoints,
                        base_hitpoints,
                    } => (
                        4,
                        128,
                        json!([amount, kind, current_hitpoints, base_hitpoints]),
                    ),
                    N::SpotAnimation {
                        animation,
                        height,
                        delay,
                    } => (5, 4, json!([animation, height, delay])),
                    N::Say { text: bytes } => (6, 32, json!(text(bytes))),
                    N::FaceCoordinate { x, z } => (7, 8, json!([x, z])),
                    N::Animation { sequence, delay } => (8, 2, json!([sequence, delay])),
                    N::Damage2 {
                        amount,
                        kind,
                        current_hitpoints,
                        base_hitpoints,
                    } => (
                        9,
                        16,
                        json!([amount, kind, current_hitpoints, base_hitpoints]),
                    ),
                };
                assert_eq!(mask & bit, 0, "duplicate decoded NPC block");
                mask |= bit;
                fields[index] = field;
            }
            fields[1] = json!(mask);
            json!(fields)
        })
        .collect::<Vec<_>>();
    json!(["NpcInfo", value.old_visible_count, old, additions, updates])
}
