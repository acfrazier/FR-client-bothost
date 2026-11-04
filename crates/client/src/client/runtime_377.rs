//! Standalone revision-377 runtime for captured Fairy Ring login and guide-dialogue packets.
//! Decode and resource validation finish before application. Unsupported families
//! fail closed; none can enter a 274/289 wire handler. Java anchors are the
//! corresponding `readPacket`, entity-info, and interface packet handlers.
use super::*;
use crate::client::packet_377::{
    decode_packet_377,
    entity_377::*,
    interface_packet_377::InterfaceOperation377 as I,
    npc_info_377::{NpcInfo377, NpcUpdateBlock377 as N},
    player_info_377::{PlayerAppearance377, PlayerInfo377, PlayerUpdateBlock377 as P},
    rebuild_377::Rebuild377,
    zone_377::ZoneOperation377 as Z,
    HintArrow377, Operation377 as O, PacketContext377, ReferenceKind377 as R,
};
use crate::dash3d::client_player::recol1d;

impl PacketContext377 for Client {
    fn contains(&self, kind: R, id: i32) -> bool {
        let Ok(index) = usize::try_from(id) else {
            return false;
        };
        match kind {
            R::Component => self.ifaces.get(index).is_some_and(Option::is_some),
            R::Object => index < self.cache.objs.len(),
            R::Npc => index < self.cache.npcs.len(),
            R::Sequence => index < self.cache.seqs.len(),
            R::IdentityKit => index < self.cache.idks.len(),
            R::NpcSlot => index < self.npc.len(),
            R::Model => self
                .on_demand
                .as_ref()
                .and_then(|od| od.version_crc(0, id))
                .is_some_and(|(v, _)| v != 0),
            R::Spotanim => index < self.cache.spots.len(),
            R::Location => index < self.cache.locs.len(),
            // Java's fixed wave-delay table exists even in low-memory mode.
            R::Sound => index < self.jagfx.delays.len(),
            R::Midi => self
                .on_demand
                .as_ref()
                .and_then(|od| od.version_crc(2, id))
                .is_some_and(|(v, _)| v != 0),
            R::Varp => index < self.cache.varps.len(),
            R::Stat => index < self.stat_xp.len(),
            R::Player => index < self.players.len(),
        }
    }
    fn inventory_capacity(&self, component: i32) -> Option<usize> {
        self.ifaces_mut
            .get(usize::try_from(component).ok()?)?
            .as_ref()?
            .link_obj_type
            .as_ref()
            .map(Vec::len)
    }
    fn visible_player_count(&self) -> Option<usize> {
        usize::try_from(self.player_count).ok()
    }
    fn visible_npc_count(&self) -> Option<usize> {
        usize::try_from(self.npc_count).ok()
    }
}

impl Client {
    pub(super) fn dispatch_packet_377(
        &mut self,
        opcode: i32,
        payload: &mut Packet,
    ) -> Result<(), String> {
        let opcode = u8::try_from(opcode).map_err(|_| "377 opcode outside byte range")?;
        // Java login, tutorial and Cook's Assistant mainland cohort.
        if !matches!(
            opcode,
            2 | 5
                | 10
                | 13
                | 21
                | 26
                | 29
                | 40
                | 49
                | 50
                | 61
                | 63
                | 71
                | 75
                | 82
                | 88
                | 90
                | 106
                | 107
                | 109
                | 113
                | 115
                | 121
                | 125
                | 126
                | 128
                | 134
                | 148
                | 152
                | 156
                | 157
                | 158
                | 159
                | 162
                | 166
                | 174
                | 181
                | 182
                | 183
                | 186
                | 199
                | 200
                | 201
                | 206
                | 208
                | 218
                | 219
                | 220
                | 222
                | 226
                | 232
                | 238
                | 249
                | 251
                | 255
        ) {
            return Err(format!(
                "377 packet {opcode}: outside captured runtime cohort"
            ));
        }
        let end = payload.frame_end().unwrap_or(payload.length());
        if end > payload.length() {
            return Err("377 frame exceeds payload storage".into());
        }
        let operation = decode_packet_377(opcode, &payload.data()[..end])
            .map_err(|e| e.to_string())?
            .validate(self)
            .map_err(|e| e.to_string())?
            .into_operation();
        self.preflight_377(&operation)?;
        let publication = self.apply_operation_377(operation);
        payload.pos = end;
        self.ptype = -1;
        if let Some(publication) = publication {
            publication.publish(&mut self.gens);
        }
        Ok(())
    }

    fn preflight_377(&self, operation: &O<'_>) -> Result<(), String> {
        match operation {
            O::PlayerInfo(info) => {
                if self.local_player.is_none() {
                    return Err("377 player frame without local player".into());
                }
                for &id in &self.player_ids[..info.old_visible_count] {
                    if self.players.get(id as usize).is_none_or(Option::is_none) {
                        return Err(format!("377 missing visible player {id}"));
                    }
                }
                unique_visible_377::<32>(
                    self.player_ids
                        .iter()
                        .copied()
                        .zip(&info.old_visible)
                        .filter(|(_, update)| !update.removed)
                        .map(|(id, _)| id)
                        .chain(info.additions.iter().map(|actor| actor.index)),
                    self.player_ids.len(),
                )?;
            }
            O::NpcInfo(info) => {
                if self.local_player.is_none() {
                    return Err("377 NPC frame without local player".into());
                }
                for &id in &self.npc_ids[..info.old_visible_count] {
                    if self.npc.get(id as usize).is_none_or(Option::is_none) {
                        return Err(format!("377 missing visible NPC {id}"));
                    }
                }
                unique_visible_377::<256>(
                    self.npc_ids
                        .iter()
                        .copied()
                        .zip(&info.old_visible)
                        .filter(|(_, update)| !update.removed)
                        .map(|(id, _)| id)
                        .chain(info.additions.iter().map(|actor| actor.index)),
                    self.npc_ids.len(),
                )?;
            }
            O::Interface(I::SetTab { tab, .. })
                if *tab < 0 || *tab as usize >= self.side_icon.len() =>
            {
                return Err("377 tab exceeds runtime capacity".into())
            }
            O::Zone(frame)
                if !(0..4).contains(&self.minusedlevel)
                    || frame.operations().iter().any(|op| {
                        !matches!(
                            op,
                            Z::LocAddChange { .. }
                                | Z::LocDel { .. }
                                | Z::ObjAdd { .. }
                                | Z::ObjDel { .. }
                                | Z::ObjCount { .. }
                                | Z::ObjectReveal { .. }
                                | Z::MapProjAnim { .. }
                        )
                    }) =>
            {
                return Err(format!(
                    "377 zone outside tutorial location/object/projectile cohort: {frame:?}"
                ))
            }
            O::Rebuild(Rebuild377::Instance { .. }) => {
                return Err("377 instance rebuild not in captured cohort".into())
            }
            O::Interface(I::SetPlayerHead { .. }) if self.local_player.is_none() => {
                return Err("377 player dialogue head without local player".into())
            }
            O::InventoryStopTransmit { component } => {
                if self
                    .if_(*component as usize)
                    .is_none_or(|com| com.link_obj_type.is_none())
                {
                    return Err("377 stopped inventory has no object array".into());
                }
            }
            O::InventoryFull { component, .. } | O::InventoryPartial { component, .. } => {
                let valid = self.if_(*component as usize).is_some_and(|com| {
                    matches!(
                        (&com.link_obj_type, &com.link_obj_number),
                        (Some(types), Some(numbers)) if types.len() == numbers.len()
                    )
                });
                if !valid {
                    return Err("377 inventory arrays have different capacities".into());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Java Component.unloadCom resets non-inventory children of a layer.
    /// The immutable decoded table stays shared; only this client's overlay
    /// returns to its decode template. No model or inventory table is copied.
    fn unload_interface_377(&mut self, layer: i32) {
        if layer == -1 {
            return;
        }
        let Some(revision) = self.cache.revision_377.as_ref() else {
            return;
        };
        let slots = Arc::make_mut(&mut self.ifaces_mut);
        for (id, component) in self.ifaces.iter().enumerate() {
            if component
                .as_ref()
                .is_some_and(|com| com.layer_id == layer && com.r#type != ComponentType::TYPE_INV)
            {
                if let (Some(slot), Some(initial)) = (
                    slots.get_mut(id),
                    revision.interfaces.initial_overlays.get(id),
                ) {
                    slot.clone_from(initial);
                }
            }
        }
    }

    fn apply_operation_377(&mut self, operation: O<'_>) -> Option<R289Publication> {
        let mut p = R289Publication::default();
        match operation {
            O::Rebuild(Rebuild377::Normal { zone_x, zone_z }) => {
                self.apply_rebuild_zones(zone_x, zone_z);
                R289Publication::ALL.publish(&mut self.gens);
                return None;
            }
            O::PlayerInfo(info) => return Some(self.apply_players_377(info)),
            O::NpcInfo(info) => return Some(self.apply_npcs_377(info)),
            O::Zone(frame) => {
                if let Some((x, z)) = frame.origin() {
                    self.zone_update_x = x;
                    self.zone_update_z = z;
                    if frame.clear() {
                        for tx in x..x + 8 {
                            for tz in z..z + 8 {
                                if (0..104).contains(&tx)
                                    && (0..104).contains(&tz)
                                    && self.ground_obj[self.minusedlevel as usize][tx as usize]
                                        [tz as usize]
                                        .take()
                                        .is_some()
                                {
                                    self.show_object(tx, tz);
                                }
                            }
                        }
                        let mut node = self.loc_changes.head();
                        while let Some(loc) = node {
                            if loc.level == self.minusedlevel
                                && (x..x + 8).contains(&loc.x)
                                && (z..z + 8).contains(&loc.z)
                            {
                                loc.end_time = 0;
                            }
                            node = self.loc_changes.next_node();
                        }
                    }
                }
                for operation in frame.operations() {
                    let tile = match operation {
                        Z::LocAddChange { tile, .. }
                        | Z::LocDel { tile, .. }
                        | Z::ObjAdd { tile, .. }
                        | Z::ObjDel { tile, .. }
                        | Z::ObjCount { tile, .. }
                        | Z::ObjectReveal { tile, .. }
                        | Z::MapProjAnim { tile, .. } => tile,
                        _ => unreachable!("preflight rejects other zone operations"),
                    };
                    let x = self.zone_update_x + tile.x;
                    let z = self.zone_update_z + tile.z;
                    if !(0..104).contains(&x) || !(0..104).contains(&z) {
                        continue;
                    }
                    match operation {
                        // Java 8658: the receiver already has the private object.
                        Z::ObjectReveal { receiver_id, .. } if *receiver_id == self.self_slot => {}
                        Z::ObjAdd { object, count, .. } | Z::ObjectReveal { object, count, .. } => {
                            // Java 8659-8666 and 8717-8733: append and sort.
                            self.ground_obj[self.minusedlevel as usize][x as usize][z as usize]
                                .get_or_insert_with(|| Box::new(LinkList::new()))
                                .push(ClientObj::new(*object, *count));
                            self.show_object(x, z);
                        }
                        Z::ObjCount {
                            object,
                            old_count,
                            new_count,
                            ..
                        } => {
                            // Java Client.java:8734-8753 matches the masked ID
                            // and old count, updates only the first match, sorts.
                            if let Some(objects) = &mut self.ground_obj[self.minusedlevel as usize]
                                [x as usize][z as usize]
                            {
                                let mut node = objects.head();
                                while let Some(obj) = node {
                                    if obj.id == *object && obj.count == *old_count {
                                        obj.count = *new_count;
                                        break;
                                    }
                                    node = objects.next_node();
                                }
                                self.show_object(x, z);
                            }
                        }
                        Z::ObjDel { object, .. } => {
                            // Java Client.java:8822-8842 removes only the first
                            // matching ID, then clears an empty stack and sorts.
                            let cell = &mut self.ground_obj[self.minusedlevel as usize][x as usize]
                                [z as usize];
                            if let Some(objects) = cell {
                                let mut node = objects.head();
                                while let Some(obj) = node {
                                    if obj.id == *object {
                                        objects.unlink_last();
                                        break;
                                    }
                                    node = objects.next_node();
                                }
                                if objects.head().is_none() {
                                    *cell = None;
                                }
                                self.show_object(x, z);
                            }
                        }
                        Z::MapProjAnim {
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
                            ..
                        } => {
                            // Java Client.java:8754-8777 consumes the 65535
                            // sentinel and requires both endpoints in the scene.
                            let (dx, dz) = (x + destination_x_offset, z + destination_z_offset);
                            if let Some(spotanim) = spotanim
                                .filter(|_| (0..104).contains(&dx) && (0..104).contains(&dz))
                            {
                                let (sx, sz, dx, dz) =
                                    (x * 128 + 64, z * 128 + 64, dx * 128 + 64, dz * 128 + 64);
                                let start = self.loop_cycle + start_delay;
                                let mut projectile = ClientProj::new(
                                    spotanim,
                                    self.minusedlevel,
                                    sx,
                                    get_av_h(&self.groundh, &self.mapl, sx, sz, self.minusedlevel)
                                        - source_height,
                                    sz,
                                    start,
                                    self.loop_cycle + end_delay,
                                    *peak,
                                    *arc,
                                    *target,
                                    *destination_height,
                                );
                                projectile.set_target(
                                    f64::from(dx),
                                    f64::from(
                                        get_av_h(
                                            &self.groundh,
                                            &self.mapl,
                                            dx,
                                            dz,
                                            self.minusedlevel,
                                        ) - destination_height,
                                    ),
                                    f64::from(dz),
                                    start,
                                );
                                projectile.bind_seq(&self.cache);
                                self.projectiles.push(projectile);
                            }
                        }
                        Z::LocAddChange {
                            shape,
                            angle,
                            location,
                            ..
                        } => {
                            self.loc_change_create(
                                self.minusedlevel,
                                x,
                                z,
                                LOC_SHAPE_TO_LAYER[*shape as usize],
                                *location,
                                *shape,
                                *angle,
                                0,
                                -1,
                            );
                        }
                        Z::LocDel { shape, angle, .. } => {
                            self.loc_change_create(
                                self.minusedlevel,
                                x,
                                z,
                                LOC_SHAPE_TO_LAYER[*shape as usize],
                                -1,
                                *shape,
                                *angle,
                                0,
                                -1,
                            );
                        }
                        _ => unreachable!("preflight rejects other zone operations"),
                    }
                }
                p.scene = true;
            }
            O::Interface(operation) => {
                match operation {
                    I::OpenChat { component } => {
                        self.if_anim_reset(component);
                        self.observe_main_modal_close();
                        if self.side_modal_id != -1 {
                            self.unload_interface_377(self.side_modal_id);
                            self.side_modal_id = -1;
                            self.redraw_side = true;
                            self.redraw_icons = true;
                        }
                        self.unload_interface_377(self.main_modal_id);
                        if self.chat_modal_id != component {
                            self.unload_interface_377(self.chat_modal_id);
                        }
                        self.chat_modal_id = component;
                        self.main_modal_id = -1;
                        self.redraw_chat = true;
                        self.resumed_pause_button = false;
                    }
                    I::SetPosition { component, x, y } => {
                        let com = self.iface_mut(component as usize).unwrap();
                        com.x = x;
                        com.y = y;
                    }
                    I::SetNpcHead { component, npc } => {
                        if let Some(com) = self.iface_mut(component as usize) {
                            com.model1_type = 2;
                            com.model1_id = npc;
                        }
                    }
                    I::SetPlayerHead { component } => {
                        // Java Client.java:7905-7915 uses Java int arithmetic
                        // for the appearance-based component model-cache key.
                        let player = self.local_player.as_ref().unwrap();
                        let head = if let Some(npc) = player.transmog {
                            (npc as i32).wrapping_add(305419896)
                        } else {
                            (i32::from(player.appearance[11]) << 5)
                                .wrapping_add(i32::from(player.appearance[8]) << 10)
                                .wrapping_add(i32::from(player.appearance[0]) << 15)
                                .wrapping_add(i32::from(player.colour[0]) << 25)
                                .wrapping_add(i32::from(player.colour[4]) << 20)
                                .wrapping_add(i32::from(player.appearance[1]))
                        };
                        let com = self.iface_mut(component as usize).unwrap();
                        com.model1_type = 3;
                        com.model1_id = head;
                    }
                    I::SetColour { component, rgb555 } => {
                        // Java Client.java:7450-7458 expands five bits per channel.
                        self.iface_mut(component as usize).unwrap().colour = ((rgb555 & 31) << 3)
                            + (((rgb555 >> 10) & 31) << 19)
                            + (((rgb555 >> 5) & 31) << 11);
                    }
                    I::SetObject {
                        component,
                        object,
                        divisor,
                    } => {
                        if object == 65535 {
                            self.iface_mut(component as usize).unwrap().model1_type = 0;
                        } else {
                            let obj = &self.cache.objs[object as usize];
                            let (xan, yan, zoom) =
                                (obj.xan2d, obj.yan2d, obj.zoom2d.wrapping_mul(100) / divisor);
                            let com = self.iface_mut(component as usize).unwrap();
                            com.model1_type = 4;
                            com.model1_id = object;
                            com.model_xan = xan;
                            com.model_yan = yan;
                            com.model_zoom = zoom;
                        }
                    }
                    I::SetAngle {
                        component,
                        xan,
                        yan,
                        zoom,
                    } => {
                        let com = self.iface_mut(component as usize).unwrap();
                        com.model_xan = xan;
                        com.model_yan = yan;
                        com.model_zoom = zoom;
                    }
                    I::SetAnim {
                        component,
                        sequence,
                    } => {
                        if let Some(com) = self.iface_mut(component as usize) {
                            if com.model_anim != sequence || sequence == -1 {
                                com.model_anim = sequence;
                                com.anim_frame = 0;
                                com.anim_cycle = 0;
                            }
                        }
                    }
                    I::SetHide { component, hidden } => {
                        if let Some(com) = self.iface_mut(component as usize) {
                            com.hide = hidden;
                        }
                    }
                    I::Close => {
                        self.unload_interface_377(self.side_modal_id);
                        self.unload_interface_377(self.chat_modal_id);
                        self.unload_interface_377(self.main_modal_id);
                        self.apply_if_close();
                    }
                    I::SetTab { tab, component } => {
                        if self.side_icon[tab as usize] != component {
                            self.unload_interface_377(self.side_icon[tab as usize]);
                        }
                        self.side_icon[tab as usize] = component;
                        self.redraw_side = true;
                        self.redraw_icons = true;
                    }
                    I::TutFlash { tab } => self.apply_tut_flash(tab),
                    I::OpenOverlay { component } => {
                        if component != -1 {
                            self.if_anim_reset(component);
                        }
                        if self.main_overlay_id != component {
                            self.unload_interface_377(self.main_overlay_id);
                        }
                        self.main_overlay_id = component;
                    }
                    I::TutOpen { component } => {
                        if self.tut_com_id != component {
                            self.unload_interface_377(self.tut_com_id);
                        }
                        self.tut_com_id = component;
                        self.redraw_chat = true;
                    }
                    I::OpenMain { component } => {
                        self.if_anim_reset(component);
                        self.observe_main_modal_open();
                        self.unload_interface_377(self.side_modal_id);
                        self.unload_interface_377(self.chat_modal_id);
                        if self.main_modal_id != component {
                            self.unload_interface_377(self.main_modal_id);
                        }
                        self.main_modal_id = component;
                        self.side_modal_id = -1;
                        self.chat_modal_id = -1;
                        self.dialog_input_open = false;
                        self.redraw_chat = true;
                        self.redraw_side = true;
                        self.redraw_icons = true;
                        self.resumed_pause_button = false;
                    }
                    I::OpenMainSide { main, side } => {
                        // Java Client.java:7741-7775 keeps unchanged layers.
                        self.observe_main_modal_open();
                        self.unload_interface_377(self.chat_modal_id);
                        if self.chat_modal_id != -1 || self.dialog_input_open {
                            self.redraw_chat = true;
                        }
                        self.chat_modal_id = -1;
                        if self.main_modal_id != main {
                            self.unload_interface_377(self.main_modal_id);
                        }
                        if self.side_modal_id != side {
                            self.unload_interface_377(self.side_modal_id);
                        }
                        self.main_modal_id = main;
                        self.side_modal_id = side;
                        self.dialog_input_open = false;
                        self.redraw_side = true;
                        self.redraw_icons = true;
                        self.resumed_pause_button = false;
                    }
                    I::SetScrollPos {
                        component,
                        position,
                    } => {
                        // Preserve Java's clamp order, including short layers.
                        let view = self.if_(component as usize).unwrap();
                        if view.r#type == ComponentType::TYPE_LAYER {
                            let position = position.max(0).min(view.scroll_height - view.height);
                            self.iface_mut(component as usize).unwrap().scroll_pos = position;
                        }
                    }
                    I::SetText { component, text } => {
                        let layer = self.if_(component as usize).unwrap().layer_id;
                        self.iface_mut(component as usize).unwrap().text = text_377(text);
                        if layer == self.side_icon[self.active_icon as usize] {
                            self.redraw_side = true;
                        }
                        // Java 8540-8549 mutates the visible component. Rust
                        // caches chat pixels across packet-pump boundaries, so
                        // a later text packet must invalidate its visible layer.
                        if layer == self.chat_modal_id || layer == self.tut_com_id {
                            self.redraw_chat = true;
                        }
                    }
                    _ => unreachable!("opcode cohort admits only captured interface operations"),
                }
                p.iface = true;
            }
            O::InventoryFull { component, entries } => {
                let inv = self.iface_mut(component as usize).unwrap();
                let types = inv.link_obj_type.as_mut().unwrap();
                let numbers = inv.link_obj_number.as_mut().unwrap();
                let count = entries.len();
                for (slot, entry) in entries.into_iter().enumerate() {
                    types[slot] = entry.object_plus_one;
                    numbers[slot] = entry.count;
                }
                types[count..].fill(0);
                numbers[count..].fill(0);
                self.redraw_side = true;
                self.observe_inventory_full(component);
                p.inv = true;
            }
            O::InventoryPartial { component, entries } => {
                let inv = self.iface_mut(component as usize).unwrap();
                let types = inv.link_obj_type.as_mut().unwrap();
                let numbers = inv.link_obj_number.as_mut().unwrap();
                for entry in entries {
                    let slot = entry.slot as usize;
                    if slot < types.len() {
                        types[slot] = entry.entry.object_plus_one;
                        numbers[slot] = entry.entry.count;
                    }
                }
                self.redraw_side = true;
                self.observe_inventory_partial(component);
                p.inv = true;
            }
            O::InventoryStopTransmit { component } => {
                // Java Client.java:8414-8424 clears IDs, not quantities.
                self.iface_mut(component as usize)
                    .unwrap()
                    .link_obj_type
                    .as_mut()
                    .unwrap()
                    .fill(0);
                p.inv = true;
            }
            O::Varp { id, value } => return Some(self.apply_varp_289(id, value)),
            O::VarpSync => return Some(self.apply_varp_sync_289()),
            O::UpdateStat {
                stat,
                level,
                experience,
            } => {
                self.apply_update_stat_289(stat, experience, level);
                p.stat = true;
            }
            O::RunEnergy(value) => {
                if self.active_icon == 12 {
                    self.redraw_side = true;
                }
                self.runenergy = value;
                p.stat = true;
            }
            O::RunWeight(value) => {
                if self.active_icon == 12 {
                    self.redraw_side = true;
                }
                self.runweight = value;
                p.stat = true;
            }
            O::UpdatePid { members, slot } => {
                self.members_account = members;
                self.self_slot = slot;
            }
            O::ResetAnims => {
                for player in self.players.iter_mut().flatten() {
                    player.primary_anim = -1;
                }
                if let Some(local) = &mut self.local_player {
                    local.primary_anim = -1;
                }
                for npc in self.npc.iter_mut().flatten() {
                    npc.primary_anim = -1;
                }
                p.player = true;
                p.npc = true;
            }
            O::Logout => {
                self.server_logout();
                return None;
            }
            O::MessageGame(bytes) => {
                let before = self.chat_seq;
                self.apply_message_game_text_289(&text_377(bytes));
                p.chat = self.chat_seq != before;
            }
            O::IgnoreList(names) => {
                self.ignore_count = names.iter().len() as i32;
                for (slot, hash) in self.ignore_userhash.iter_mut().zip(names.iter()) {
                    *slot = hash;
                }
            }
            O::FriendListLoaded(value) => {
                self.friend_server_status = value;
                self.redraw_side = true;
            }
            O::ChatFilter {
                public,
                private,
                trade,
            } => {
                self.chat_public_mode = public;
                self.chat_private_mode = private;
                self.chat_trade_mode = trade;
                self.redraw_chat = true;
                self.redraw_chat_mode = true;
            }
            O::PlayerOption {
                index,
                text,
                priority,
            } => {
                if (1..=5).contains(&index) {
                    let text = text_377(text);
                    self.player_op[(index - 1) as usize] =
                        (!text.eq_ignore_ascii_case("null")).then_some(text);
                    self.player_op_priority[(index - 1) as usize] = priority == 0;
                }
            }
            O::SynthSound {
                sound,
                loops,
                delay,
            } => {
                // Java Client.java:7303-7323. 65535 bypasses lowmem/mute.
                if self.wave_count < 50
                    && (delay == 65535 || self.wave_enabled && !self.config.lowmem)
                {
                    let slot = self.wave_count as usize;
                    self.wave_ids[slot] = if delay == 65535 {
                        i32::from(sound as i16)
                    } else {
                        sound
                    };
                    self.wave_loops[slot] = loops;
                    self.wave_delay[slot] = if delay == 65535 {
                        0
                    } else {
                        delay + self.jagfx.delays[sound as usize]
                    };
                    self.wave_count += 1;
                }
            }
            O::MidiSong(song) => {
                if self.next_midi_song != song
                    && self.midi_active
                    && !self.config.lowmem
                    && self.next_music_delay == 0
                {
                    self.midi_song = song;
                    self.midi_fading = true;
                    if let Some(od) = &mut self.on_demand {
                        od.request(2, song);
                    }
                }
                self.next_midi_song = song;
            }
            O::MidiJingle { song, delay } => {
                // Java Client.java:7424-7435 leaves the background song intact.
                if self.midi_active && !self.config.lowmem {
                    self.midi_song = song;
                    self.midi_fading = false;
                    if let Some(od) = &mut self.on_demand {
                        od.request(2, song);
                    }
                    self.next_music_delay = delay;
                }
            }
            O::MinimapToggle(value) => self.minimap_state = value,
            O::UnsetMapFlag => {
                self.minimap_flag_x = 0;
                self.try_move_path.clear();
                p.map_flag = true;
            }
            O::CameraReset => {
                self.cinema_cam = false;
                self.cam_shake.fill(false);
                p.camera = true;
            }
            O::Hint(hint) => {
                match hint {
                    HintArrow377::Npc { slot, .. } => {
                        self.hint_type = 1;
                        self.hint_npc = slot;
                    }
                    HintArrow377::Player { slot, .. } => {
                        self.hint_type = 10;
                        self.hint_player = slot;
                    }
                    HintArrow377::Tile { kind, x, z, height } => {
                        (self.hint_offset_x, self.hint_offset_z) = match kind {
                            2 => (64, 64),
                            3 => (0, 64),
                            4 => (128, 64),
                            5 => (64, 0),
                            6 => (64, 128),
                            _ => unreachable!(),
                        };
                        self.hint_type = 2;
                        self.hint_tile_x = x;
                        self.hint_tile_z = z;
                        self.hint_height = height;
                    }
                    HintArrow377::Other { kind, .. } => self.hint_type = kind,
                }
                p.world = true;
            }
            _ => unreachable!("operation outside captured 377 cohort"),
        }
        Some(p)
    }

    fn apply_players_377(&mut self, mut info: PlayerInfo377<'_>) -> R289Publication {
        // Resolve ordinals before compacting the existing list, without copying actors or IDs.
        for update in &mut info.updates {
            if let ActorReference377::OldVisible(ordinal) = update.actor {
                update.actor = ActorReference377::New(self.player_ids[ordinal]);
            }
        }
        self.entity_removal_count = 0;
        for ordinal in 0..self.player_count as usize {
            if ordinal >= info.old_visible_count || info.old_visible[ordinal].removed {
                self.entity_removal_ids[self.entity_removal_count as usize] =
                    self.player_ids[ordinal];
                self.entity_removal_count += 1;
            }
        }
        let cycle = self.loop_cycle;
        if let Some(movement) = &info.local_movement {
            self.move_actor_377(false, LOCAL_PLAYER_INDEX as usize, movement);
        }
        let mut count = 0;
        for (ordinal, update) in info.old_visible.iter().enumerate() {
            let id = self.player_ids[ordinal] as usize;
            if update.removed {
                continue;
            }
            self.player_ids[count] = id as i32;
            count += 1;
            self.players[id].as_mut().unwrap().cycle = cycle;
            if let Some(movement) = &update.movement {
                self.move_actor_377(false, id, movement);
            }
        }
        for addition in &info.additions {
            let id = addition.index as usize;
            if self.players[id].is_none() {
                let mut player = Box::<ClientPlayer>::default();
                if let Some(raw) = &self.player_appearance_buffer[id] {
                    let appearance =
                        crate::client::packet_377::player_info_377::decode_cached_appearance_377(
                            raw.data(),
                        )
                        .expect("previously validated 377 appearance");
                    apply_appearance_377(&mut player, &appearance);
                }
                self.players[id] = Some(player);
            }
            self.players[id].as_mut().unwrap().cycle = cycle;
            self.player_ids[count] = id as i32;
            count += 1;
            self.move_actor_377(false, id, &addition.movement);
        }
        let mut p = R289Publication {
            player: true,
            player_info: true,
            ..Default::default()
        };
        for update in &info.updates {
            let id = resolve_actor_377(update.actor);
            for block in &info.blocks[update.blocks.clone()] {
                p.chat |= self.player_block_377(id, block);
            }
        }
        for ordinal in 0..self.entity_removal_count as usize {
            let id = self.entity_removal_ids[ordinal] as usize;
            if self.players[id]
                .as_ref()
                .is_some_and(|player| player.cycle != cycle)
            {
                self.players[id] = None;
            }
        }
        self.player_count = count as i32;
        self.awaiting_player_info = false;
        p
    }

    fn apply_npcs_377(&mut self, mut info: NpcInfo377<'_>) -> R289Publication {
        for update in &mut info.updates {
            if let ActorReference377::OldVisible(ordinal) = update.actor {
                update.actor = ActorReference377::New(self.npc_ids[ordinal]);
            }
        }
        self.entity_removal_count = 0;
        for ordinal in 0..self.npc_count as usize {
            if ordinal >= info.old_visible_count || info.old_visible[ordinal].removed {
                self.entity_removal_ids[self.entity_removal_count as usize] = self.npc_ids[ordinal];
                self.entity_removal_count += 1;
            }
        }
        let cycle = self.loop_cycle;
        let mut count = 0;
        for (ordinal, update) in info.old_visible.iter().enumerate() {
            let id = self.npc_ids[ordinal] as usize;
            if update.removed {
                continue;
            }
            self.npc_ids[count] = id as i32;
            count += 1;
            self.npc[id].as_mut().unwrap().cycle = cycle;
            if let Some(movement) = &update.movement {
                self.move_actor_377(true, id, movement);
            }
        }
        for addition in &info.additions {
            let id = addition.index as usize;
            let npc = self.npc[id].get_or_insert_with(Box::<ClientNpc>::default);
            bind_npc_377(npc, addition.definition_id as usize, &self.cache);
            npc.cycle = cycle;
            self.npc_ids[count] = id as i32;
            count += 1;
            self.move_actor_377(true, id, &addition.movement);
        }
        for update in &info.updates {
            let id = resolve_actor_377(update.actor);
            for block in &info.blocks[update.blocks.clone()] {
                self.npc_block_377(id, block);
            }
        }
        for ordinal in 0..self.entity_removal_count as usize {
            let id = self.entity_removal_ids[ordinal] as usize;
            if self.npc[id].as_ref().is_some_and(|npc| npc.cycle != cycle) {
                self.npc[id] = None;
            }
        }
        self.npc_count = count as i32;
        R289Publication {
            npc: true,
            npc_info: true,
            ..Default::default()
        }
    }

    fn move_actor_377(&mut self, npc: bool, id: usize, movement: &ActorMovement377) {
        if let ActorMovement377::Teleport {
            plane: Some(plane), ..
        } = movement
        {
            self.minusedlevel = *plane;
        }
        let (local_x, local_z) = self
            .local_player
            .as_ref()
            .map(|p| (p.route_x[0], p.route_z[0]))
            .unwrap();
        let cache = &self.cache;
        let entity = if npc {
            &mut self.npc[id].as_mut().unwrap().entity
        } else if id == LOCAL_PLAYER_INDEX as usize {
            &mut self.local_player.as_mut().unwrap().entity
        } else {
            &mut self.players[id].as_mut().unwrap().entity
        };
        match *movement {
            ActorMovement377::Step {
                first_direction,
                second_direction,
            } => {
                entity.move_code(cache, second_direction.is_some(), first_direction);
                if let Some(second) = second_direction {
                    entity.move_code(cache, true, second);
                }
            }
            ActorMovement377::Teleport { x, z, jump, .. } => entity.teleport(cache, jump, x, z),
            ActorMovement377::Offset { dx, dz, jump } => {
                entity.teleport(cache, jump, local_x + dx, local_z + dz)
            }
        }
    }

    fn player_377(&mut self, id: usize) -> &mut ClientPlayer {
        if id == LOCAL_PLAYER_INDEX as usize {
            self.local_player.as_mut().unwrap()
        } else {
            self.players[id].as_deref_mut().unwrap()
        }
    }

    fn player_block_377(&mut self, id: usize, block: &P<'_>) -> bool {
        let cycle = self.loop_cycle;
        match block {
            P::Appearance(appearance) => {
                apply_appearance_377(self.player_377(id), appearance);
                self.player_appearance_buffer[id] =
                    Some(Box::new(Packet::new(appearance.raw.wire.to_vec())));
            }
            P::Animation { sequence, delay } => {
                let cache = self.cache.clone();
                animate_377(
                    &mut self.player_377(id).entity,
                    sequence.unwrap_or(-1),
                    *delay,
                    &cache,
                );
                if id == LOCAL_PLAYER_INDEX as usize {
                    self.local_animation_update = Some(LocalAnimationUpdate::next(
                        self.local_animation_update,
                        sequence.unwrap_or(-1),
                    ));
                }
            }
            P::FaceEntity(target) => {
                self.player_377(id).face_entity = if target.target.is_none() {
                    -1
                } else {
                    target.wire_value
                }
            }
            P::FaceCoordinate { x, z } => {
                let p = self.player_377(id);
                p.face_square_x = *x;
                p.face_square_z = *z;
            }
            P::SpotAnimation {
                animation,
                height,
                delay,
            } => spot_377(
                &mut self.player_377(id).entity,
                animation.unwrap_or(-1),
                *height,
                *delay,
                cycle,
            ),
            P::Damage {
                amount,
                kind,
                current_hitpoints,
                base_hitpoints,
            }
            | P::Damage2 {
                amount,
                kind,
                current_hitpoints,
                base_hitpoints,
            } => hit_377(
                &mut self.player_377(id).entity,
                *amount,
                *kind,
                *current_hitpoints,
                *base_hitpoints,
                cycle,
            ),
            P::ExactMove {
                start_x,
                start_z,
                end_x,
                end_z,
                start_delay,
                end_delay,
                direction,
            } => {
                let p = self.player_377(id);
                p.exact_start_x = *start_x;
                p.exact_start_z = *start_z;
                p.exact_end_x = *end_x;
                p.exact_end_z = *end_z;
                p.exact_move_end = cycle + start_delay;
                p.exact_move_start = cycle + end_delay;
                p.exact_move_facing = *direction;
                p.abort_route();
            }
            P::Say { text } => {
                let text = text_377(text);
                let log = text.starts_with('~') || id == LOCAL_PLAYER_INDEX as usize;
                let text = text.strip_prefix('~').unwrap_or(&text).to_owned();
                let player = self.player_377(id);
                player.chat_message = Some(text.clone());
                player.chat_colour = 0;
                player.chat_effect = 0;
                player.chat_timer = 150;
                if log {
                    let name = player.name.clone().unwrap_or_default();
                    self.add_chat(2, &text, &name);
                    return true;
                }
            }
            P::Chat {
                colour,
                effect,
                rights,
                packed,
            } => {
                let player = self.player_377(id);
                if let (Some(name), true) = (player.name.clone(), player.ready) {
                    let hash = JString::to_userhash(&name) as i64;
                    if self.chat_disabled == 0
                        && (*rights > 1
                            || !self.ignore_userhash[..self.ignore_count as usize].contains(&hash))
                    {
                        let mut packed = Packet::new(packed.decoded_iter().collect());
                        let length = packed.length();
                        let text = WordFilter::filter(&WordPack::unpack(&mut packed, length));
                        let player = self.player_377(id);
                        player.chat_message = Some(text.clone());
                        player.chat_colour = *colour;
                        player.chat_effect = *effect;
                        player.chat_timer = 150;
                        let (kind, name) = match rights {
                            2 | 3 => (1, format!("@cr2@{name}")),
                            1 => (1, format!("@cr1@{name}")),
                            _ => (2, name),
                        };
                        self.add_chat(kind, &text, &name);
                        return true;
                    }
                }
            }
        }
        false
    }

    fn npc_block_377(&mut self, id: usize, block: &N<'_>) {
        let cycle = self.loop_cycle;
        let npc = self.npc[id].as_mut().unwrap();
        match block {
            N::ChangeType { definition_id } => {
                bind_npc_377(npc, *definition_id as usize, &self.cache)
            }
            N::Animation { sequence, delay } => {
                animate_377(&mut npc.entity, sequence.unwrap_or(-1), *delay, &self.cache)
            }
            N::FaceEntity(target) => {
                npc.face_entity = if target.target.is_none() {
                    -1
                } else {
                    target.wire_value
                }
            }
            N::FaceCoordinate { x, z } => {
                npc.face_square_x = *x;
                npc.face_square_z = *z;
            }
            N::SpotAnimation {
                animation,
                height,
                delay,
            } => spot_377(
                &mut npc.entity,
                animation.unwrap_or(-1),
                *height,
                *delay,
                cycle,
            ),
            N::Damage {
                amount,
                kind,
                current_hitpoints,
                base_hitpoints,
            }
            | N::Damage2 {
                amount,
                kind,
                current_hitpoints,
                base_hitpoints,
            } => hit_377(
                &mut npc.entity,
                *amount,
                *kind,
                *current_hitpoints,
                *base_hitpoints,
                cycle,
            ),
            N::Say { text } => {
                npc.chat_message = Some(text_377(text));
                npc.chat_timer = 100;
            }
        }
    }
}

fn unique_visible_377<const WORDS: usize>(
    ids: impl Iterator<Item = i32>,
    capacity: usize,
) -> Result<(), String> {
    let mut seen = [0u64; WORDS];
    for (count, id) in ids.enumerate() {
        let id = usize::try_from(id).map_err(|_| "negative 377 visible actor")?;
        if count >= capacity || id / 64 >= WORDS || seen[id / 64] & (1 << (id % 64)) != 0 {
            return Err("377 duplicate actor or visible-list capacity exceeded".into());
        }
        seen[id / 64] |= 1 << (id % 64);
    }
    Ok(())
}

fn resolve_actor_377(actor: ActorReference377) -> usize {
    match actor {
        ActorReference377::LocalPlayer => LOCAL_PLAYER_INDEX as usize,
        ActorReference377::New(slot) => slot as usize,
        ActorReference377::OldVisible(_) => unreachable!("resolved before visible-list compaction"),
    }
}
fn text_377(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}
fn bind_npc_377(npc: &mut ClientNpc, id: usize, cache: &Cache) {
    let kind = cache.npc(id);
    npc.r#type = Some(id);
    npc.size = kind.size;
    npc.turnspeed = kind.turnspeed;
    npc.walkanim = kind.walkanim;
    npc.walkanim_b = kind.walkanim_b;
    npc.walkanim_l = kind.walkanim_l;
    npc.walkanim_r = kind.walkanim_r;
    npc.readyanim = kind.readyanim;
}
fn apply_appearance_377(player: &mut ClientPlayer, appearance: &PlayerAppearance377<'_>) {
    player.gender = appearance.gender;
    // This renderer uses an icon bitset; Java 377 sends a signed single index.
    player.headicons = if appearance.head_icon < 0 {
        0
    } else {
        1 << appearance.head_icon
    };
    for (slot, value) in player.appearance.iter_mut().zip(appearance.equipment) {
        if let Some(value) = value {
            *slot = value as u16;
        }
    }
    player.transmog = appearance.npc_transform.map(|id| id as usize);
    for (i, &colour) in appearance.body_colours.iter().enumerate() {
        player.colour[i] = if colour >= 0 && (colour as usize) < recol1d()[i].len() {
            colour as u16
        } else {
            0
        };
    }
    [
        player.readyanim,
        player.turnanim,
        player.walkanim,
        player.walkanim_b,
        player.walkanim_r,
        player.walkanim_l,
        player.runanim,
    ] = appearance.sequences.map(|id| id.unwrap_or(-1));
    player.name = Some(JString::to_screen_name(&JString::to_raw_username(
        appearance.username_base37,
    )));
    player.combat_level = appearance.combat_level;
    player.skill_level = appearance.skill_level;
    player.ready = true;
    let mut base = 0i64;
    for slot in 0..12 {
        let part = player.appearance[match slot {
            5 => 9,
            9 => 5,
            _ => slot,
        }];
        base = base.wrapping_shl(4);
        if part >= 256 {
            base = base.wrapping_add(part as i64 - 256);
        }
    }
    if player.appearance[0] >= 256 {
        base = base.wrapping_add((player.appearance[0] as i64 - 256) >> 4);
    }
    if player.appearance[1] >= 256 {
        base = base.wrapping_add((player.appearance[1] as i64 - 256) >> 8);
    }
    for colour in player.colour {
        base = base.wrapping_shl(3).wrapping_add(colour as i64);
    }
    player.base_id = base.wrapping_shl(1).wrapping_add(player.gender as i64);
}
fn animate_377(
    entity: &mut crate::dash3d::client_entity::ClientEntity,
    id: i32,
    delay: i32,
    cache: &Cache,
) {
    if entity.primary_anim == id && id != -1 {
        match cache.seq(id as usize).duplicatebehaviour {
            1 => {
                entity.primary_anim_frame = 0;
                entity.primary_anim_cycle = 0;
                entity.primary_anim_delay = delay;
                entity.primary_anim_loop = 0;
            }
            2 => entity.primary_anim_loop = 0,
            _ => {}
        }
    } else if id == -1
        || entity.primary_anim == -1
        || cache.seq(id as usize).priority >= cache.seq(entity.primary_anim as usize).priority
    {
        entity.primary_anim = id;
        entity.primary_anim_frame = 0;
        entity.primary_anim_cycle = 0;
        entity.primary_anim_delay = delay;
        entity.primary_anim_loop = 0;
        entity.preanim_route_length = entity.route_length;
    }
}
fn spot_377(
    entity: &mut crate::dash3d::client_entity::ClientEntity,
    id: i32,
    height: i32,
    delay: i32,
    cycle: i32,
) {
    entity.spotanim_id = id;
    entity.spotanim_height = height;
    entity.spotanim_last_cycle = cycle + delay;
    entity.spotanim_frame = if delay > 0 { -1 } else { 0 };
    entity.spotanim_cycle = 0;
}
fn hit_377(
    entity: &mut crate::dash3d::client_entity::ClientEntity,
    amount: i32,
    kind: i32,
    health: i32,
    max: i32,
    cycle: i32,
) {
    entity.add_hitmark(cycle, kind, amount);
    entity.combat_cycle = cycle + 300;
    entity.health = health;
    entity.total_health = max;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_with_cache(cache: Cache) -> Client {
        let mut client = Client::from_shared_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43595,
                cache_dir: "/nonexistent/t2a-377-cache".into(),
                members: true,
                lowmem: true,
            },
            Arc::new(cache),
            Arc::new(Vec::new()),
            Arc::new(Vec::new()),
            ClientRevision::R377,
        );
        client.ingame = true;
        client.local_player = Some(ClientPlayer::default());
        client
    }

    fn client() -> Client {
        client_with_cache(Cache::default())
    }

    #[test]
    fn complete_frame_validation_precedes_any_apply() {
        let mut client = client();
        client.chat_public_mode = 7;
        let mut short = Packet::new(vec![1, 2]);
        short.set_frame_end(2);
        assert!(client.dispatch_packet_377(201, &mut short).is_err());
        assert_eq!(client.chat_public_mode, 7);
        assert!(client.ingame);
        assert_eq!(short.pos, 0);
        let mut valid_but_outside_cohort = Packet::new(vec![]);
        assert!(client
            .dispatch_packet_377(60, &mut valid_but_outside_cohort)
            .unwrap_err()
            .contains("outside captured"));
    }

    #[test]
    fn tutorial_projectile_matches_java_and_rejects_invalid_frames_before_apply() {
        let mut cache = Cache::default();
        cache.spots.push(Default::default());
        let mut client = client_with_cache(cache);
        client.zone_update_x = 8;
        client.zone_update_z = 16;
        client.loop_cycle = 100;
        // Java 8756-8768: tile(2,3), destination(+2,-1), NPC slot 0,
        // spot 0, heights 3*4/5*4, delays 4/12, peak 16, arc 8.
        let bytes = [0x23, 2, 255, 0, 1, 0, 0, 3, 5, 0, 4, 0, 12, 16, 8];
        let generation = client.gens.scene;
        for len in 0..bytes.len() {
            let mut packet = Packet::new(bytes[..len].to_vec());
            assert!(client.dispatch_packet_377(181, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert!(client.projectiles.head().is_none());
            assert_eq!(client.gens.scene, generation);
        }
        let mut invalid = bytes;
        invalid[6] = 1; // Missing spot resource.
        let mut packet = Packet::new(invalid.to_vec());
        assert!(client.dispatch_packet_377(181, &mut packet).is_err());
        assert_eq!(packet.pos, 0);
        assert!(client.projectiles.head().is_none());
        assert_eq!(client.gens.scene, generation);
        // A late invalid enclosed child cannot publish the earlier projectile
        // or change the zone origin.
        let mut enclosed = vec![24, 160, 181];
        enclosed.extend(bytes);
        enclosed.push(181);
        enclosed.extend(invalid);
        let mut packet = Packet::new(enclosed);
        assert!(client.dispatch_packet_377(183, &mut packet).is_err());
        assert_eq!(packet.pos, 0);
        assert_eq!((client.zone_update_x, client.zone_update_z), (8, 16));
        assert!(client.projectiles.head().is_none());
        assert_eq!(client.gens.scene, generation);
        let mut packet = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(181, &mut packet).unwrap();
        assert_eq!(packet.pos, bytes.len());
        let projectile = client.projectiles.head().unwrap();
        assert_eq!((projectile.src_x, projectile.src_z), (1344, 2496));
        assert_eq!((projectile.h1, projectile.h2), (-12, 20));
        assert_eq!((projectile.t1, projectile.t2), (104, 112));
        assert_eq!(
            (projectile.angle, projectile.startpos, projectile.target),
            (16, 8, 1)
        );
        assert_eq!(projectile.spotanim, 0);
        assert!(projectile.velocity_x > 0.0 && projectile.velocity_z < 0.0);
        assert!(projectile.acceleration_y.is_finite());
        assert_eq!(client.gens.scene, generation + 1);
    }

    #[test]
    fn tutorial_projectile_sentinel_and_outside_endpoints_are_consumed_without_entities() {
        let mut cache = Cache::default();
        cache.spots.push(Default::default());
        let mut client = client_with_cache(cache);
        for bytes in [
            [
                0x23, 2, 255, 0x7f, 0xff, 0xff, 0xff, 3, 5, 0, 4, 0, 12, 16, 8,
            ],
            [0x23, 127, 0, 0, 0, 0, 0, 3, 5, 0, 4, 0, 12, 16, 8],
            [0x23, 0x80, 0, 0, 0, 0, 0, 3, 5, 0, 4, 0, 12, 16, 8],
        ] {
            let mut packet = Packet::new(bytes.to_vec());
            client.dispatch_packet_377(181, &mut packet).unwrap();
            assert_eq!(packet.pos, bytes.len());
            assert!(client.projectiles.head().is_none());
        }
    }

    #[test]
    fn tutorial_inventory_weight_is_signed_and_validates_before_apply() {
        let mut client = client();
        for (tab, redraw) in [(3, false), (12, true)] {
            client.active_icon = tab;
            client.redraw_side = false;
            let generation = client.gens.stat;
            let mut weight = Packet::new(vec![0xff, 0xfe]);
            client.dispatch_packet_377(174, &mut weight).unwrap();
            assert_eq!(client.runweight, -2);
            assert_eq!(client.redraw_side, redraw);
            assert_eq!(client.gens.stat, generation + 1);
        }
        let generation = client.gens.stat;
        let mut short = Packet::new(vec![1]);
        assert!(client.dispatch_packet_377(174, &mut short).is_err());
        assert_eq!(client.runweight, -2);
        assert_eq!(client.gens.stat, generation);
        assert_eq!(short.pos, 0);
    }

    #[test]
    fn large_tutorial_varp_validates_before_publishing() {
        let mut cache = Cache::default();
        cache.varps.resize_with(2, Default::default);
        let mut client = client_with_cache(cache);
        let generation = client.gens.varp;
        // Java Client.java:7574 and Packet.java:414: g4_alt3, then g2_alt1.
        let mut large = Packet::new(vec![0x34, 0x12, 0x78, 0x56, 1, 0]);
        client.dispatch_packet_377(115, &mut large).unwrap();
        assert_eq!(client.var_serv[1], 0x12345678);
        assert_eq!(client.var[1], 0x12345678);
        assert_eq!(client.gens.varp, generation + 1);
        assert_eq!(large.pos, 6);

        let generation = client.gens.varp;
        for body in [
            vec![0x34, 0x12, 0x78, 0x56, 2, 0],
            vec![0x34, 0x12, 0x78, 0x56, 1],
        ] {
            let mut invalid = Packet::new(body);
            assert!(client.dispatch_packet_377(115, &mut invalid).is_err());
            assert_eq!(invalid.pos, 0);
            assert_eq!(client.var_serv[1], 0x12345678);
            assert_eq!(client.var[1], 0x12345678);
            assert_eq!(client.gens.varp, generation);
        }
    }

    #[test]
    fn tutorial_location_packets_schedule_real_scene_changes_after_validation() {
        let mut cache = Cache::default();
        cache.locs.push(Default::default());
        let mut client = client_with_cache(cache);
        let mut origin = Packet::new(vec![248, 144]);
        client.dispatch_packet_377(75, &mut origin).unwrap();
        assert_eq!((client.zone_update_x, client.zone_update_z), (8, 16));
        // shape 10, angle 1, loc 0 and zone offset (2,3).
        let mut add = Packet::new(vec![215, 128, 0, 163]);
        client.dispatch_packet_377(152, &mut add).unwrap();
        {
            let change = client.loc_changes.head().unwrap();
            assert_eq!(
                (change.level, change.layer, change.x, change.z),
                (0, 2, 10, 19)
            );
            assert_eq!(
                (change.new_type, change.new_shape, change.new_angle),
                (0, 10, 1)
            );
        }
        let generation = client.gens.scene;
        let mut invalid = Packet::new(vec![215, 129, 0, 163]);
        assert!(client.dispatch_packet_377(152, &mut invalid).is_err());
        assert_eq!(client.gens.scene, generation);
        let mut delete = Packet::new(vec![93, 87]);
        client.dispatch_packet_377(88, &mut delete).unwrap();
        let change = client.loc_changes.head().unwrap();
        assert_eq!(
            (change.new_type, change.new_shape, change.new_angle),
            (-1, 10, 1)
        );
    }

    #[test]
    fn firemaking_ground_logs_add_delete_and_validate_transactionally() {
        let mut cache = Cache::default();
        cache.objs.resize_with(2, Default::default);
        let mut client = client_with_cache(cache);
        client.zone_update_x = 8;
        client.zone_update_z = 16;
        // Java g2(object), g1_alt2(tile 2,3), g2_alt2(count).
        let mut add = Packet::new(vec![0, 1, 221, 0, 131]);
        let generation = client.gens.scene;
        client.dispatch_packet_377(107, &mut add).unwrap();
        assert_eq!(client.gens.scene, generation + 1);
        let objects = client.ground_obj[0][10][19].as_mut().unwrap();
        let obj = objects.head().unwrap();
        assert_eq!((obj.id, obj.count), (1, 3));
        assert!(objects.next_node().is_none());
        assert_eq!(
            client.world.ground_object_at(0, 10, 19).unwrap().top,
            Some((1, 3))
        );

        // A second same-ID node survives the first deletion.
        let mut duplicate = Packet::new(vec![0, 1, 221, 0, 132]);
        client.dispatch_packet_377(107, &mut duplicate).unwrap();
        let generation = client.gens.scene;
        for (opcode, bytes) in [
            (107, vec![0, 2, 221, 0, 129]),
            (107, vec![0, 1, 221, 0]),
            (208, vec![0, 130, 163]),
            (208, vec![0, 129]),
        ] {
            let mut invalid = Packet::new(bytes);
            assert!(client.dispatch_packet_377(opcode, &mut invalid).is_err());
            assert_eq!(invalid.pos, 0);
            assert_eq!(client.gens.scene, generation);
        }
        // Deletion masks bit 15 and uses g1_alt1(tile).
        let mut delete = Packet::new(vec![128, 129, 163]);
        client.dispatch_packet_377(208, &mut delete).unwrap();
        let objects = client.ground_obj[0][10][19].as_mut().unwrap();
        assert_eq!(objects.head().unwrap().id, 1);
        assert!(objects.next_node().is_none());
        let mut delete = Packet::new(vec![0, 129, 163]);
        client.dispatch_packet_377(208, &mut delete).unwrap();
        assert!(client.ground_obj[0][10][19].is_none());
        assert!(client.world.ground_object_at(0, 10, 19).is_none());
    }

    #[test]
    fn tutorial_ground_count_matches_old_count_and_is_transactional() {
        let mut cache = Cache::default();
        cache.objs.push(Default::default());
        let mut client = client_with_cache(cache);
        client.zone_update_x = 8;
        client.zone_update_z = 16;
        let mut objects = LinkList::new();
        for count in [5, 5, 4] {
            objects.push(ClientObj::new(0, count));
        }
        client.ground_obj[0][10][19] = Some(Box::new(objects));
        fn counts(client: &mut Client) -> Vec<i32> {
            let objects = client.ground_obj[0][10][19].as_mut().unwrap();
            let mut values = Vec::new();
            let mut node = objects.head();
            while let Some(obj) = node {
                values.push(obj.count);
                node = objects.next_node();
            }
            values.sort_unstable();
            values
        }
        // Java 8736-8747: packed tile, masked object 0, old 5, new 9.
        let bytes = [0x23, 0x80, 0, 0, 5, 0, 9];
        let generation = client.gens.scene;
        for len in 0..bytes.len() {
            let mut packet = Packet::new(bytes[..len].to_vec());
            assert!(client.dispatch_packet_377(121, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(counts(&mut client), [4, 5, 5]);
            assert_eq!(client.gens.scene, generation);
        }
        let mut invalid = bytes;
        invalid[1] = 0;
        invalid[2] = 1;
        let mut batch = vec![8, 144, 121];
        batch.extend(bytes);
        batch.push(121);
        batch.extend(invalid);
        let mut packet = Packet::new(batch);
        assert!(client.dispatch_packet_377(183, &mut packet).is_err());
        assert_eq!(packet.pos, 0);
        assert_eq!(counts(&mut client), [4, 5, 5]);
        assert_eq!(client.gens.scene, generation);
        let mut packet = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(121, &mut packet).unwrap();
        assert_eq!(packet.pos, bytes.len());
        assert_eq!(counts(&mut client), [4, 5, 9]);
        assert_eq!(client.gens.scene, generation + 1);
        let mut no_match = Packet::new(vec![0x23, 0, 0, 0, 8, 0, 7]);
        client.dispatch_packet_377(121, &mut no_match).unwrap();
        assert_eq!(counts(&mut client), [4, 5, 9]);
        let mut absent = Packet::new(vec![0x22, 0, 0, 0, 5, 0, 9]);
        client.dispatch_packet_377(121, &mut absent).unwrap();
        assert!(client.ground_obj[0][10][18].is_none());
    }

    #[test]
    fn tutorial_object_reveal_matches_java_receiver_and_scene_guards() {
        let mut cache = Cache::default();
        cache.objs.resize_with(2, Default::default);
        let mut client = client_with_cache(cache);
        client.self_slot = 7;
        client.zone_update_x = 8;
        client.zone_update_z = 16;
        // Java 8652-8657: tile (2,3), count 4, object 1, receiver 8.
        let bytes = [163, 132, 0, 0, 129, 0, 136];
        let generation = client.gens.scene;
        for len in 0..bytes.len() {
            let mut packet = Packet::new(bytes[..len].to_vec());
            assert!(client.dispatch_packet_377(106, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.scene, generation);
            assert!(client.ground_obj[0][10][19].is_none());
        }
        for invalid in [
            vec![163, 132, 0, 0, 130, 0, 136],   // missing object 2
            vec![163, 132, 0, 0, 129, 255, 126], // invalid receiver 65534
        ] {
            let mut packet = Packet::new(invalid);
            assert!(client.dispatch_packet_377(106, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.scene, generation);
            assert!(client.ground_obj[0][10][19].is_none());
        }
        let mut packet = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(106, &mut packet).unwrap();
        assert_eq!(packet.pos, bytes.len());
        assert_eq!(client.gens.scene, generation + 1);
        assert_eq!(
            client.world.ground_object_at(0, 10, 19).unwrap().top,
            Some((1, 4))
        );
        // The owner already sees the private object. Do not duplicate it.
        let mut owner = Packet::new(vec![163, 137, 0, 0, 129, 0, 135]);
        client.dispatch_packet_377(106, &mut owner).unwrap();
        assert_eq!(owner.pos, bytes.len());
        let objects = client.ground_obj[0][10][19].as_mut().unwrap();
        let obj = objects.head().unwrap();
        assert_eq!((obj.id, obj.count), (1, 4));
        assert!(objects.next_node().is_none());
        // Java compares the broadcast sentinel with the PID like any receiver.
        let mut broadcast = Packet::new(vec![163, 137, 0, 0, 129, 255, 127]);
        client.dispatch_packet_377(106, &mut broadcast).unwrap();
        assert_eq!(broadcast.pos, bytes.len());
        let objects = client.ground_obj[0][10][19].as_mut().unwrap();
        let mut counts = Vec::new();
        let mut node = objects.head();
        while let Some(obj) = node {
            assert_eq!(obj.id, 1);
            counts.push(obj.count);
            node = objects.next_node();
        }
        counts.sort_unstable();
        assert_eq!(counts, [4, 9]);
        client.zone_update_x = 103;
        client.zone_update_z = 103;
        let mut outside = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(106, &mut outside).unwrap();
        assert_eq!(outside.pos, bytes.len());
        assert!(client.ground_obj[0][103][103].is_none());
    }

    #[test]
    fn tutorial_enclosed_object_reveal_validates_whole_observed_batch() {
        let mut cache = Cache::default();
        cache.objs.resize_with(2531, Default::default);
        let mut client = client_with_cache(cache);
        client.self_slot = 7;
        client.zone_update_x = 8;
        client.zone_update_z = 16;
        // Reconstructed from the decoded live frame: origin (64,64), tile
        // (5,6), two count-1 objects 2530/2134, receiver 2. Not a raw capture.
        let bytes = [
            64, 192, 106, 214, 129, 0, 9, 98, 0, 130, 106, 214, 129, 0, 8, 214, 0, 130,
        ];
        let mut invalid_object = bytes;
        invalid_object[14] = 10;
        let mut invalid_receiver = bytes;
        invalid_receiver[16] = 8;
        invalid_receiver[17] = 128;
        let generation = client.gens.scene;
        for invalid in [
            bytes[..17].to_vec(),
            invalid_object.to_vec(),
            invalid_receiver.to_vec(),
        ] {
            let mut packet = Packet::new(invalid);
            assert!(client.dispatch_packet_377(183, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.scene, generation);
            assert_eq!((client.zone_update_x, client.zone_update_z), (8, 16));
            assert!(client.ground_obj[0][69][70].is_none());
        }
        let mut packet = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(183, &mut packet).unwrap();
        assert_eq!(packet.pos, bytes.len());
        assert_eq!((client.zone_update_x, client.zone_update_z), (64, 64));
        assert_eq!(client.gens.scene, generation + 1);
        let objects = client.ground_obj[0][69][70].as_mut().unwrap();
        let mut values = Vec::new();
        let mut node = objects.head();
        while let Some(obj) = node {
            values.push((obj.id, obj.count));
            node = objects.next_node();
        }
        values.sort_unstable();
        assert_eq!(values, [(2134, 1), (2530, 1)]);
        assert!(client.world.ground_object_at(0, 69, 70).is_some());
    }

    #[test]
    fn enclosed_firemaking_batch_rejects_late_invalid_and_unsupported_children() {
        let mut cache = Cache::default();
        cache.objs.push(Default::default());
        cache.spots.push(Default::default());
        let mut client = client_with_cache(cache);
        for suffix in [
            vec![107, 0, 1, 221, 0, 129], // invalid second object
            vec![59, 35, 0, 0, 0, 0, 2],  // valid resources, outside accepted apply cohort
        ] {
            let mut batch = vec![8, 144, 107, 0, 0, 221, 0, 129];
            batch.extend(suffix);
            let mut packet = Packet::new(batch);
            let generation = client.gens.scene;
            assert!(client.dispatch_packet_377(183, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.scene, generation);
            assert_eq!((client.zone_update_x, client.zone_update_z), (0, 0));
            assert!(client.ground_obj[0][10][19].is_none());
        }
        let mut packet = Packet::new(vec![8, 144, 107, 0, 0, 221, 0, 129, 208, 0, 128, 163]);
        client.dispatch_packet_377(183, &mut packet).unwrap();
        assert_eq!((client.zone_update_x, client.zone_update_z), (8, 16));
        assert!(client.ground_obj[0][10][19].is_none());
        assert!(client.world.ground_object_at(0, 10, 19).is_none());
    }

    #[test]
    fn chef_player_dialogue_head_matches_java_hash_and_validates_before_apply() {
        let mut client = client();
        client.set_iface(301, IfType::default());
        let player = client.local_player.as_mut().unwrap();
        player.appearance[11] = 257;
        player.appearance[8] = 258;
        player.appearance[0] = 259;
        player.appearance[1] = 260;
        player.colour[0] = 2;
        player.colour[4] = 3;
        let mut packet = Packet::new(vec![173, 1]);
        let generation = client.gens.iface;
        client.dispatch_packet_377(255, &mut packet).unwrap();
        assert_eq!(packet.pos, 2);
        assert_eq!(client.gens.iface, generation + 1);
        let component = client.if_(301).unwrap();
        assert_eq!((component.model1_type, component.model1_id), (3, 79014180));
        client.local_player.as_mut().unwrap().transmog = Some(943);
        let mut packet = Packet::new(vec![173, 1]);
        client.dispatch_packet_377(255, &mut packet).unwrap();
        assert_eq!(client.if_(301).unwrap().model1_id, 305420839);
        let generation = client.gens.iface;
        for bytes in [vec![173], vec![128, 255]] {
            let mut invalid = Packet::new(bytes);
            assert!(client.dispatch_packet_377(255, &mut invalid).is_err());
            assert_eq!(invalid.pos, 0);
            assert_eq!(client.gens.iface, generation);
            assert_eq!(client.if_(301).unwrap().model1_id, 305420839);
        }
        client.local_player = None;
        let mut invalid = Packet::new(vec![173, 1]);
        assert!(client.dispatch_packet_377(255, &mut invalid).is_err());
        assert_eq!(invalid.pos, 0);
        assert_eq!(client.gens.iface, generation);
        assert_eq!(client.if_(301).unwrap().model1_id, 305420839);
    }

    #[test]
    fn open_chat_unloads_replaced_layers_but_retains_inventory_and_same_chat() {
        let mut client = client();
        for (id, layer, kind) in [
            (200, -1, ComponentType::TYPE_LAYER),
            (201, 200, ComponentType::TYPE_TEXT),
            (202, 200, ComponentType::TYPE_INV),
            (210, -1, ComponentType::TYPE_LAYER),
            (211, 210, ComponentType::TYPE_MODEL),
            (220, -1, ComponentType::TYPE_LAYER),
            (221, 220, ComponentType::TYPE_TEXT),
            (300, -1, ComponentType::TYPE_LAYER),
            (301, 300, ComponentType::TYPE_TEXT),
        ] {
            client.set_iface(
                id,
                IfType {
                    id: id as i32,
                    layer_id: layer,
                    r#type: kind,
                    ..Default::default()
                },
            );
            client.iface_mut(id).unwrap().text = format!("decoded {id}");
        }
        client.iface_mut(202).unwrap().link_obj_type = Some(vec![0, 0]);
        client.iface_mut(202).unwrap().link_obj_number = Some(vec![0, 0]);
        Arc::get_mut(&mut client.cache).unwrap().revision_377 =
            Some(Box::new(crate::config::CacheRevision377 {
                config: Default::default(),
                interfaces: crate::config::runtime_377::InterfaceFields377 {
                    client_code_layers: Default::default(),
                    type1_fields: Vec::new(),
                    null_string_masks: Vec::new(),
                    initial_overlays: client.ifaces_mut.as_ref().clone(),
                },
            }));
        for id in [201, 211, 221, 301] {
            let com = client.iface_mut(id).unwrap();
            com.text = "live change".into();
            com.anim_frame = 8;
            com.anim_cycle = 9;
            com.model1_id = 55;
            com.hide = true;
        }
        client.iface_mut(202).unwrap().link_obj_type = Some(vec![11, 22]);
        client.main_modal_id = 200;
        client.side_modal_id = 210;
        client.chat_modal_id = 220;
        let mut open = Packet::new(vec![1, 44]);
        client.dispatch_packet_377(109, &mut open).unwrap();
        assert_eq!(client.chat_modal_id, 300);
        assert_eq!((client.main_modal_id, client.side_modal_id), (-1, -1));
        for id in [201, 211, 221] {
            let com = client.if_(id).unwrap();
            assert_eq!(com.text, format!("decoded {id}"));
            assert_eq!((com.anim_frame, com.anim_cycle, com.model1_id), (0, 0, 0));
            assert!(!com.hide);
        }
        assert_eq!(
            client.if_(202).unwrap().link_obj_type.as_deref(),
            Some(&[11, 22][..])
        );
        assert_eq!(client.if_(301).unwrap().text, "live change");
        let mut same = Packet::new(vec![1, 44]);
        client.dispatch_packet_377(109, &mut same).unwrap();
        assert_eq!(client.if_(301).unwrap().text, "live change");
        let mut reopen = Packet::new(vec![0, 220]);
        client.dispatch_packet_377(109, &mut reopen).unwrap();
        assert_eq!(client.if_(301).unwrap().text, "decoded 301");
        assert_eq!(client.if_(221).unwrap().text, "decoded 221");
    }

    #[test]
    fn late_text_packet_invalidates_only_its_visible_cached_layer() {
        let mut client = client();
        client.side_icon[3] = 300;
        client.active_icon = 3;
        client.chat_modal_id = 400;
        client.tut_com_id = 500;
        let bytes = [173, 1, b't', b'i', b'n', 10]; // component 301, Java g2_alt3.
        for (layer, side, chat) in [
            (300, true, false),
            (400, false, true),
            (500, false, true),
            (600, false, false),
        ] {
            client.set_iface(
                301,
                IfType {
                    layer_id: layer,
                    ..Default::default()
                },
            );
            client.redraw_side = false;
            client.redraw_chat = false;
            let generation = client.gens.iface;
            let mut packet = Packet::new(bytes.to_vec());
            client.dispatch_packet_377(232, &mut packet).unwrap();
            assert_eq!(client.if_(301).unwrap().text, "tin");
            assert_eq!((client.redraw_side, client.redraw_chat), (side, chat));
            assert_eq!(client.gens.iface, generation + 1);
        }
        client.redraw_chat = false;
        client.redraw_side = false;
        let generation = client.gens.iface;
        for len in 0..bytes.len() {
            let mut packet = Packet::new(bytes[..len].to_vec());
            assert!(client.dispatch_packet_377(232, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.if_(301).unwrap().text, "tin");
            assert!(!client.redraw_chat && !client.redraw_side);
            assert_eq!(client.gens.iface, generation);
        }
    }

    #[test]
    fn survival_inventory_full_partial_and_validation_are_transactional() {
        let mut cache = Cache::default();
        cache.objs.resize_with(2, Default::default);
        let mut client = client_with_cache(cache);
        client.set_iface(300, IfType::default());
        let inv = client.iface_mut(300).unwrap();
        inv.link_obj_type = Some(vec![9; 3]);
        inv.link_obj_number = Some(vec![9; 3]);
        // Java 8065-8085: LE biased IDs, negated quantities, LE large count.
        let mut full = Packet::new(vec![1, 44, 0, 2, 129, 0, 255, 130, 0, 1, 244, 1, 0, 0]);
        client.dispatch_packet_377(206, &mut full).unwrap();
        assert_eq!(
            client.if_(300).unwrap().link_obj_type.as_deref(),
            Some(&[1, 2, 0][..])
        );
        assert_eq!(
            client.if_(300).unwrap().link_obj_number.as_deref(),
            Some(&[1, 500, 0][..])
        );
        let generation = client.gens.inv;
        let mut partial = Packet::new(vec![1, 44, 2, 0, 2, 4]);
        client.dispatch_packet_377(134, &mut partial).unwrap();
        assert_eq!(
            client.if_(300).unwrap().link_obj_type.as_deref(),
            Some(&[1, 2, 2][..])
        );
        assert_eq!(
            client.if_(300).unwrap().link_obj_number.as_deref(),
            Some(&[1, 500, 4][..])
        );
        assert!(client.gens.inv > generation);
        let generation = client.gens.inv;
        let mut invalid = Packet::new(vec![1, 44, 0, 0, 2, 9, 2, 0, 3, 1]);
        assert!(client.dispatch_packet_377(134, &mut invalid).is_err());
        assert_eq!(invalid.pos, 0);
        assert_eq!(client.gens.inv, generation);
        assert_eq!(
            client.if_(300).unwrap().link_obj_number.as_deref(),
            Some(&[1, 500, 4][..])
        );
    }

    #[test]
    fn survival_object_display_angles_and_tab_flash_match_java() {
        let mut cache = Cache::default();
        cache.objs.push(ObjType {
            xan2d: 100,
            yan2d: 200,
            zoom2d: 2000,
            ..Default::default()
        });
        let mut client = client_with_cache(cache);
        client.set_iface(301, IfType::default());
        let mut object = Packet::new(vec![0, 250, 0, 0, 173, 1]);
        client.dispatch_packet_377(21, &mut object).unwrap();
        let mut position = Packet::new(vec![10, 0, 236, 255, 1, 45]);
        client.dispatch_packet_377(166, &mut position).unwrap();
        assert_eq!(
            (client.if_(301).unwrap().x, client.if_(301).unwrap().y),
            (-20, 10)
        );
        let com = client.if_(301).unwrap();
        assert_eq!((com.model1_type, com.model1_id), (4, 0));
        assert_eq!(
            (com.model_xan, com.model_yan, com.model_zoom),
            (100, 200, 800)
        );
        let mut angle = Packet::new(vec![1, 172, 173, 1, 3, 104, 144, 1]);
        client.dispatch_packet_377(186, &mut angle).unwrap();
        let com = client.if_(301).unwrap();
        assert_eq!(
            (com.model_xan, com.model_yan, com.model_zoom),
            (300, 400, 1000)
        );
        let mut invalid = Packet::new(vec![0, 0, 0, 0, 173, 1]);
        assert!(client.dispatch_packet_377(21, &mut invalid).is_err());
        assert_eq!(client.if_(301).unwrap().model_zoom, 1000);
        client.active_icon = 3;
        let mut flash = Packet::new(vec![3]);
        client.dispatch_packet_377(238, &mut flash).unwrap();
        assert_eq!((client.active_icon, client.tut_flash_icon), (1, 3));
        assert!(client.redraw_side);
    }

    #[test]
    fn quest_jingle_validates_then_obeys_java_lowmem_and_mute_guards() {
        use std::io::Write;
        // A whole-compressed versionlist keeps the actual OnDemand reference
        // validator in the packet path without a cache or network dependency.
        let files: [(&str, &[u8]); 8] = [
            ("model_version", &[0, 1]),
            ("anim_version", &[0, 1]),
            ("midi_version", &[0, 1]),
            ("map_version", &[0, 1]),
            ("model_crc", &[0; 4]),
            ("anim_crc", &[0; 4]),
            ("midi_crc", &[0; 4]),
            ("map_crc", &[0; 4]),
        ];
        let mut body = Packet::new(vec![0; 256]);
        body.p2(files.len() as i32);
        for (name, data) in files {
            body.p4(JagFile::gen_hash(name));
            body.p3(data.len() as i32);
            body.p3(data.len() as i32);
        }
        for (_, data) in files {
            body.pdata(data, 0, data.len());
        }
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(&body.data()[..body.pos]).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut archive = Packet::new(vec![0; compressed.len() + 6]);
        archive.p3(body.pos as i32);
        archive.p3((compressed.len() - 4) as i32);
        archive.pdata(&compressed[4..], 0, compressed.len() - 4);
        let versionlist = JagFile::new(archive.data()[..archive.pos].to_vec());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut od = OnDemand::new_with_revision(
            &versionlist,
            "127.0.0.1",
            listener.local_addr().unwrap().port(),
            "",
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ClientRevision::R377,
        )
        .unwrap();
        od.stop(); // Retain tables and the client queue, without a worker.
        let mut client = client();
        client.on_demand = Some(od);
        client.midi_song = 7;
        client.next_midi_song = 8;
        client.next_music_delay = 9;
        client.midi_fading = true;
        let bytes = [0, 0, 0x34, 0x12, 0x56]; // song 0, delay 0x123456.
        for invalid in (0..bytes.len())
            .map(|len| bytes[..len].to_vec())
            .chain([vec![1, 0, 0x34, 0x12, 0x56]])
        {
            let mut packet = Packet::new(invalid);
            assert!(client.dispatch_packet_377(249, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!((client.midi_song, client.next_music_delay), (7, 9));
            assert!(client.midi_fading);
            assert_eq!(client.on_demand.as_ref().unwrap().remaining(), 0);
        }
        for (lowmem, active) in [(true, true), (false, false)] {
            client.config.lowmem = lowmem;
            client.midi_active = active;
            let mut packet = Packet::new(bytes.to_vec());
            client.dispatch_packet_377(249, &mut packet).unwrap();
            assert_eq!(packet.pos, bytes.len());
            assert_eq!((client.midi_song, client.next_music_delay), (7, 9));
            assert!(client.midi_fading);
            assert_eq!(client.on_demand.as_ref().unwrap().remaining(), 0);
        }
        client.config.lowmem = false;
        client.midi_active = true;
        let mut packet = Packet::new(bytes.to_vec());
        client.dispatch_packet_377(249, &mut packet).unwrap();
        assert_eq!(packet.pos, bytes.len());
        assert_eq!(client.midi_song, 0);
        assert_eq!(client.next_music_delay, 0x123456);
        assert_eq!(client.next_midi_song, 8);
        assert!(!client.midi_fading);
        assert_eq!(client.on_demand.as_ref().unwrap().remaining(), 1);
    }

    #[test]
    fn synth_sound_respects_java_lowmem_and_sentinel_branches() {
        let mut client = client();
        let mut quiet = Packet::new(vec![0, 0, 1, 0, 18]);
        client.dispatch_packet_377(26, &mut quiet).unwrap();
        assert_eq!(client.wave_count, 0);
        client.config.lowmem = false;
        client.wave_enabled = true;
        Arc::make_mut(&mut client.jagfx.delays)[0] = 7;
        let mut sound = Packet::new(vec![0, 0, 2, 0, 18]);
        client.dispatch_packet_377(26, &mut sound).unwrap();
        assert_eq!(client.wave_count, 1);
        assert_eq!(
            (
                client.wave_ids[0],
                client.wave_loops[0],
                client.wave_delay[0]
            ),
            (0, 2, 25)
        );
        client.config.lowmem = true;
        client.wave_enabled = false;
        let mut sentinel = Packet::new(vec![255, 255, 1, 255, 255]);
        client.dispatch_packet_377(26, &mut sentinel).unwrap();
        assert_eq!(client.wave_count, 2);
        assert_eq!((client.wave_ids[1], client.wave_delay[1]), (-1, 0));
    }

    #[test]
    fn dialogue_pages_apply_heads_animation_text_and_reset_continue_latch() {
        let mut cache = Cache::default();
        cache.npcs.push(Default::default());
        cache.seqs.push(Default::default());
        let mut client = client_with_cache(cache);

        let page = IfType {
            children: Some(vec![301]),
            ..Default::default()
        };
        client.set_iface(300, page);
        client.set_iface(301, IfType::default());
        client.set_iface(302, IfType::default());
        client.set_iface(303, IfType::default());
        client.iface_mut(301).unwrap().model_anim = 7;
        client.iface_mut(301).unwrap().anim_frame = 4;
        client.iface_mut(301).unwrap().anim_cycle = 5;

        let mut head = Packet::new(vec![0, 128, 45, 1]);
        client.dispatch_packet_377(162, &mut head).unwrap();
        let mut animation = Packet::new(vec![173, 1, 0, 128]);
        client.dispatch_packet_377(2, &mut animation).unwrap();
        let mut text =
            Packet::new([vec![174, 1], b"Hello, adventurer!".to_vec(), vec![10]].concat());
        client.dispatch_packet_377(232, &mut text).unwrap();
        let mut hide = Packet::new(vec![1, 1, 45]);
        client.dispatch_packet_377(82, &mut hide).unwrap();

        let component = client.iface_mut(301).unwrap();
        assert_eq!((component.model1_type, component.model1_id), (2, 0));
        assert_eq!(
            (
                component.model_anim,
                component.anim_frame,
                component.anim_cycle
            ),
            (0, 0, 0)
        );
        assert!(client.iface_mut(301).unwrap().hide);
        assert_eq!(client.iface_mut(302).unwrap().text, "Hello, adventurer!");

        client.iface_mut(301).unwrap().anim_frame = 8;
        client.iface_mut(301).unwrap().anim_cycle = 9;
        client.main_modal_id = 40;
        client.side_modal_id = 41;
        client.chat_modal_id = 42;
        client.resumed_pause_button = true;
        let iface_generation = client.gens.iface;
        let mut first_page = Packet::new(vec![1, 44]);
        client.dispatch_packet_377(109, &mut first_page).unwrap();
        assert_eq!(client.chat_modal_id, 300);
        assert_eq!(client.main_modal_id, -1);
        assert_eq!(client.side_modal_id, -1);
        assert!(!client.resumed_pause_button);
        assert_eq!(client.gens.iface, iface_generation + 1);
        let component = client.iface_mut(301).unwrap();
        assert_eq!((component.model1_type, component.model1_id), (2, 0));
        assert_eq!(
            (
                component.model_anim,
                component.anim_frame,
                component.anim_cycle
            ),
            (0, 0, 0)
        );

        client.resumed_pause_button = true;
        let mut second_page = Packet::new(vec![1, 47]);
        client.dispatch_packet_377(109, &mut second_page).unwrap();
        assert_eq!(client.chat_modal_id, 303);
        assert!(!client.resumed_pause_button);
    }

    #[test]
    fn invalid_dialogue_resource_fails_before_any_mutation() {
        let mut cache = Cache::default();
        cache.npcs.push(Default::default());
        let mut client = client_with_cache(cache);
        client.set_iface(301, IfType::default());
        let component = client.iface_mut(301).unwrap();
        component.model1_type = 4;
        component.model1_id = 55;
        component.model_anim = 7;
        component.anim_frame = 8;
        component.anim_cycle = 9;
        client.main_modal_id = 40;
        client.side_modal_id = 41;
        client.chat_modal_id = 42;
        client.resumed_pause_button = true;
        client.ptype = 73;
        let iface_generation = client.gens.iface;

        let mut invalid_head = Packet::new(vec![0, 129, 45, 1]);
        assert!(client.dispatch_packet_377(162, &mut invalid_head).is_err());

        let component = client.iface_mut(301).unwrap();
        assert_eq!((component.model1_type, component.model1_id), (4, 55));
        assert_eq!(
            (
                component.model_anim,
                component.anim_frame,
                component.anim_cycle
            ),
            (7, 8, 9)
        );
        assert_eq!(client.main_modal_id, 40);
        assert_eq!(client.side_modal_id, 41);
        assert_eq!(client.chat_modal_id, 42);
        assert!(client.resumed_pause_button);
        assert_eq!(client.ptype, 73);
        assert_eq!(invalid_head.pos, 0);
        assert_eq!(client.gens.iface, iface_generation);
    }

    #[test]
    fn quest_colour_decodes_validates_then_applies_java_rgb555() {
        let mut client = client();
        client.set_iface(301, IfType::default());
        client.iface_mut(301).unwrap().colour = 7;
        let generation = client.gens.iface;
        // IF_SETCOLOUR: component BE, RGB555 BE with biased low byte.
        let mut valid = Packet::new(vec![1, 45, 0x7c, 0x80]);
        client.dispatch_packet_377(218, &mut valid).unwrap();
        assert_eq!(client.if_(301).unwrap().colour, 0xf80000);
        assert_eq!(valid.pos, 4);
        assert_eq!(client.gens.iface, generation + 1);
        let mut channels = Packet::new(vec![1, 45, 0x04, 0xc3]);
        client.dispatch_packet_377(218, &mut channels).unwrap();
        assert_eq!(client.if_(301).unwrap().colour, 0x081018);
        let generation = client.gens.iface;
        for bytes in [vec![1, 46, 0x7c, 0x80], vec![1, 45, 0x7c]] {
            let mut invalid = Packet::new(bytes);
            assert!(client.dispatch_packet_377(218, &mut invalid).is_err());
            assert_eq!(invalid.pos, 0);
            assert_eq!(client.if_(301).unwrap().colour, 0x081018);
            assert_eq!(client.gens.iface, generation);
        }
    }

    #[test]
    fn bank_main_side_validates_both_components_before_opening() {
        let mut client = client();
        client.set_iface(301, IfType::default());
        client.set_iface(302, IfType::default());
        client.main_modal_id = 17;
        client.side_modal_id = 18;
        client.chat_modal_id = 19;
        client.dialog_input_open = true;
        client.resumed_pause_button = true;
        let generation = client.gens.iface;
        for bytes in [vec![1, 173, 175, 1], vec![1, 173, 174]] {
            let mut invalid = Packet::new(bytes);
            assert!(client.dispatch_packet_377(128, &mut invalid).is_err());
            assert_eq!(invalid.pos, 0);
            assert_eq!(
                (
                    client.main_modal_id,
                    client.side_modal_id,
                    client.chat_modal_id
                ),
                (17, 18, 19)
            );
            assert!(client.resumed_pause_button && client.dialog_input_open);
            assert_eq!(client.gens.iface, generation);
        }
        let mut valid = Packet::new(vec![1, 173, 174, 1]);
        client.dispatch_packet_377(128, &mut valid).unwrap();
        assert_eq!(
            (
                client.main_modal_id,
                client.side_modal_id,
                client.chat_modal_id
            ),
            (301, 302, -1)
        );
        assert!(!client.dialog_input_open && !client.resumed_pause_button);
        assert!(client.redraw_chat && client.redraw_side && client.redraw_icons);
        assert_eq!(client.gens.iface, generation + 1);
    }

    #[test]
    fn bank_scroll_clamps_and_non_layer_is_unchanged() {
        let mut client = client();
        client.set_iface(
            301,
            IfType {
                r#type: ComponentType::TYPE_LAYER,
                height: 100,
                ..Default::default()
            },
        );
        client.iface_mut(301).unwrap().scroll_height = 300;
        for (wire, expected) in [(150, 150), (500, 200)] {
            let mut packet = Packet::new(vec![
                1,
                45,
                (wire as u8).wrapping_add(128),
                (wire >> 8) as u8,
            ]);
            client.dispatch_packet_377(200, &mut packet).unwrap();
            assert_eq!(client.if_(301).unwrap().scroll_pos, expected);
        }
        let generation = client.gens.iface;
        for bytes in [vec![1, 46, 128, 0], vec![1, 45, 128]] {
            let mut packet = Packet::new(bytes);
            assert!(client.dispatch_packet_377(200, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.iface, generation);
            assert_eq!(client.if_(301).unwrap().scroll_pos, 200);
        }
        client.set_iface(302, IfType::default());
        client.iface_mut(302).unwrap().scroll_pos = 27;
        client
            .dispatch_packet_377(200, &mut Packet::new(vec![1, 46, 128, 0]))
            .unwrap();
        assert_eq!(client.if_(302).unwrap().scroll_pos, 27);
    }

    #[test]
    fn smithing_inventory_stop_clears_ids_but_keeps_java_counts() {
        let mut client = client();
        client.set_iface(301, IfType::default());
        let inventory = client.iface_mut(301).unwrap();
        inventory.link_obj_type = Some(vec![2, 3]);
        inventory.link_obj_number = Some(vec![7, 8]);
        let generation = client.gens.inv;
        for bytes in [vec![46, 1], vec![45]] {
            let mut packet = Packet::new(bytes);
            assert!(client.dispatch_packet_377(219, &mut packet).is_err());
            assert_eq!(packet.pos, 0);
            assert_eq!(client.gens.inv, generation);
            assert_eq!(
                client.if_(301).unwrap().link_obj_type.as_deref(),
                Some(&[2, 3][..])
            );
        }
        let mut packet = Packet::new(vec![45, 1]);
        client.dispatch_packet_377(219, &mut packet).unwrap();
        assert_eq!(packet.pos, 2);
        assert_eq!(
            client.if_(301).unwrap().link_obj_type.as_deref(),
            Some(&[0, 0][..])
        );
        assert_eq!(
            client.if_(301).unwrap().link_obj_number.as_deref(),
            Some(&[7, 8][..])
        );
        assert_eq!(client.gens.inv, generation + 1);
    }

    #[test]
    fn unset_map_flag_clears_the_visible_route_and_publishes() {
        let mut client = client();
        client.minimap_flag_x = 12;
        client.try_move_path = vec![(10, 20), (11, 20)];
        let map_flag_generation = client.gens.map_flag;
        let mut packet = Packet::new(vec![]);

        client.dispatch_packet_377(61, &mut packet).unwrap();

        assert_eq!(client.minimap_flag_x, 0);
        assert!(client.try_move_path.is_empty());
        assert_eq!(client.gens.map_flag, map_flag_generation + 1);
    }

    #[test]
    fn old_visible_mask_resolves_before_list_compaction() {
        let mut client = client();
        client.loop_cycle = 10;
        client.player_count = 2;
        client.player_ids[..2].copy_from_slice(&[5, 8]);
        client.players[5] = Some(Box::<ClientPlayer>::default());
        client.players[8] = Some(Box::<ClientPlayer>::default());
        let info = PlayerInfo377 {
            local_movement: None,
            old_visible_count: 2,
            old_visible: vec![
                OldVisibleActor377 {
                    movement: None,
                    removed: true,
                },
                OldVisibleActor377 {
                    movement: None,
                    removed: false,
                },
            ],
            additions: vec![],
            updates: vec![
                crate::client::packet_377::player_info_377::PlayerActorUpdate377 {
                    actor: ActorReference377::OldVisible(1),
                    blocks: 0..1,
                },
            ],
            blocks: vec![P::Say { text: b"retained" }],
        };
        let operation = O::PlayerInfo(info);
        client.preflight_377(&operation).unwrap();
        let O::PlayerInfo(info) = operation else {
            unreachable!()
        };
        client.apply_players_377(info);
        assert_eq!(client.player_count, 1);
        assert_eq!(client.player_ids[0], 8);
        assert!(client.players[5].is_none());
        assert_eq!(
            client.players[8].as_ref().unwrap().chat_message.as_deref(),
            Some("retained")
        );
    }

    struct CaptureReplay377 {
        client: Client,
        frame_count: usize,
        dialogue_pages: usize,
        head_updates: usize,
        animations: usize,
        text_updates: usize,
        hide_updates: usize,
        map_flag_resets: usize,
        interface_closes: usize,
    }

    fn replay_capture(cache_dir: &str, capture_path: &str, check_spawn: bool) -> CaptureReplay377 {
        let (cache, ifaces, overlays) =
            Client::load_cache_revision(cache_dir, true, ClientRevision::R377).unwrap();
        let versionlist = JagFile::new(std::fs::read(format!("{cache_dir}/versionlist")).unwrap());
        let model_versions = versionlist.read("model_version").unwrap();
        let midi_versions = versionlist.read("midi_version").unwrap();
        struct Context<'a> {
            client: &'a Client,
            models: &'a [u8],
            midi: &'a [u8],
        }
        impl PacketContext377 for Context<'_> {
            fn contains(&self, kind: R, id: i32) -> bool {
                let table = match kind {
                    R::Model => Some(self.models),
                    R::Midi => Some(self.midi),
                    _ => None,
                };
                if let Some(table) = table {
                    return usize::try_from(id)
                        .ok()
                        .and_then(|id| table.get(id * 2..id * 2 + 2))
                        .is_some_and(|bytes| bytes != [0, 0]);
                }
                self.client.contains(kind, id)
            }
            fn inventory_capacity(&self, id: i32) -> Option<usize> {
                self.client.inventory_capacity(id)
            }
            fn visible_player_count(&self) -> Option<usize> {
                self.client.visible_player_count()
            }
            fn visible_npc_count(&self) -> Option<usize> {
                self.client.visible_npc_count()
            }
        }
        let mut client = Client::from_shared_with_revision(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43595,
                cache_dir: cache_dir.into(),
                members: true,
                lowmem: true,
            },
            Arc::new(cache),
            Arc::new(ifaces),
            Arc::new(overlays),
            ClientRevision::R377,
        );
        client.ingame = true;
        client.local_player = Some(ClientPlayer::default());
        let capture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(capture_path).unwrap()).unwrap();
        let frames = capture["inbound"].as_array().unwrap();
        let mut replay = CaptureReplay377 {
            client,
            frame_count: frames.len(),
            dialogue_pages: 0,
            head_updates: 0,
            animations: 0,
            text_updates: 0,
            hide_updates: 0,
            map_flag_resets: 0,
            interface_closes: 0,
        };
        for frame in frames {
            let opcode = frame["opcode"].as_u64().unwrap() as u8;
            replay.dialogue_pages += (opcode == 109) as usize;
            replay.head_updates += (opcode == 162) as usize;
            replay.animations += (opcode == 2) as usize;
            replay.text_updates += (opcode == 232) as usize;
            replay.hide_updates += (opcode == 82) as usize;
            replay.map_flag_resets += (opcode == 61) as usize;
            replay.interface_closes += (opcode == 29) as usize;
            let bytes: Vec<u8> = frame["payload"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect();
            let context = Context {
                client: &replay.client,
                models: &model_versions,
                midi: &midi_versions,
            };
            let operation = decode_packet_377(opcode, &bytes)
                .unwrap()
                .validate(&context)
                .unwrap()
                .into_operation();
            replay.client.preflight_377(&operation).unwrap();
            let open_chat = matches!(&operation, O::Interface(I::OpenChat { .. }));
            let close_interface = matches!(&operation, O::Interface(I::Close));
            if open_chat {
                replay.client.resumed_pause_button = true;
            }
            if check_spawn && opcode == 5 {
                let local = replay.client.local_player.as_ref().unwrap();
                assert!(local.ready);
                assert_eq!(
                    (
                        replay.client.map_build_base_x + local.route_x[0],
                        replay.client.map_build_base_z + local.route_z[0]
                    ),
                    (3094, 3106)
                );
                assert!(replay.client.npc_count > 0);
            }
            if let Some(publication) = replay.client.apply_operation_377(operation) {
                publication.publish(&mut replay.client.gens);
            }
            if open_chat || close_interface {
                assert!(!replay.client.resumed_pause_button);
            }
            if close_interface {
                assert_eq!(replay.client.chat_modal_id, -1);
            }
        }
        replay
    }

    #[test]
    fn actual_engine_login_capture_replays_against_its_cache() {
        let (Ok(cache_dir), Ok(capture)) = (
            std::env::var("CLIENT_377_RUNTIME_CACHE_DIR"),
            std::env::var("CLIENT_377_CAPTURE"),
        ) else {
            eprintln!(
                "377 captured replay not provisioned: set CLIENT_377_RUNTIME_CACHE_DIR and CLIENT_377_CAPTURE"
            );
            return;
        };
        let mut replay = replay_capture(&cache_dir, &capture, true);
        assert!(!replay.client.ingame);
        assert!(matches!(
            replay.client.take_session_exit_reason(),
            Some(SessionExitReason::ServerLogout { .. })
        ));
        eprintln!(
            "377 replayed {} actual engine frames with actual cache references; no socket, asset delivery, or scene2 rendering claimed",
            replay.frame_count
        );
    }

    #[test]
    fn actual_engine_interaction_capture_replays_dialogue_cohort() {
        let (Ok(cache_dir), Ok(capture)) = (
            std::env::var("CLIENT_377_RUNTIME_CACHE_DIR"),
            std::env::var("CLIENT_377_INTERACTION_CAPTURE"),
        ) else {
            eprintln!(
                "377 interaction replay not provisioned: set CLIENT_377_RUNTIME_CACHE_DIR and CLIENT_377_INTERACTION_CAPTURE"
            );
            return;
        };
        let mut replay = replay_capture(&cache_dir, &capture, false);
        assert!(replay.dialogue_pages >= 5);
        assert!(replay.head_updates > 0);
        assert!(replay.animations > 0);
        assert!(replay.text_updates > 0);
        assert!(replay.hide_updates > 0);
        assert!(replay.map_flag_resets > 0);
        assert!(replay.interface_closes > 0);
        assert_eq!(replay.client.chat_modal_id, -1);
        assert!(!replay.client.resumed_pause_button);
        assert!(!replay.client.ingame);
        assert!(matches!(
            replay.client.take_session_exit_reason(),
            Some(SessionExitReason::ServerLogout { .. })
        ));
        eprintln!(
            "377 replayed {} captured dialogue interaction frames; {} IF_OPENCHAT pages applied",
            replay.frame_count, replay.dialogue_pages
        );
    }
}
