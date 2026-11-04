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
            // This cohort contains no synth/area-sound packets.
            R::Sound => false,
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
        // This is the exact observed login/logout/dialogue cohort, not a numeric alias table.
        if !matches!(
            opcode,
            2 | 5
                | 10
                | 13
                | 29
                | 40
                | 49
                | 50
                | 61
                | 63
                | 71
                | 82
                | 90
                | 109
                | 113
                | 125
                | 126
                | 148
                | 156
                | 157
                | 158
                | 159
                | 162
                | 182
                | 199
                | 201
                | 220
                | 222
                | 226
                | 232
                | 251
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
                if !frame.operations().is_empty() || !(0..4).contains(&self.minusedlevel) =>
            {
                return Err("377 zone outside captured origin-only cohort".into())
            }
            O::Rebuild(Rebuild377::Instance { .. }) => {
                return Err("377 instance rebuild not in captured cohort".into())
            }
            _ => {}
        }
        Ok(())
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
                p.scene = true;
            }
            O::Interface(operation) => {
                match operation {
                    I::OpenChat { component } => {
                        self.if_anim_reset(component);
                        self.observe_main_modal_close();
                        if self.side_modal_id != -1 {
                            self.side_modal_id = -1;
                            self.redraw_side = true;
                            self.redraw_icons = true;
                        }
                        self.chat_modal_id = component;
                        self.main_modal_id = -1;
                        self.redraw_chat = true;
                        self.resumed_pause_button = false;
                    }
                    I::SetNpcHead { component, npc } => {
                        if let Some(com) = self.iface_mut(component as usize) {
                            com.model1_type = 2;
                            com.model1_id = npc;
                        }
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
                    I::Close => self.apply_if_close(),
                    I::SetTab { tab, component } => {
                        self.side_icon[tab as usize] = component;
                        self.redraw_side = true;
                        self.redraw_icons = true;
                    }
                    I::OpenOverlay { component } => {
                        if component != -1 {
                            self.if_anim_reset(component);
                        }
                        self.main_overlay_id = component;
                    }
                    I::TutOpen { component } => {
                        self.tut_com_id = component;
                        self.redraw_chat = true;
                    }
                    I::OpenMain { component } => {
                        self.if_anim_reset(component);
                        self.observe_main_modal_open();
                        self.main_modal_id = component;
                        self.side_modal_id = -1;
                        self.chat_modal_id = -1;
                        self.dialog_input_open = false;
                        self.redraw_chat = true;
                        self.redraw_side = true;
                        self.redraw_icons = true;
                        self.resumed_pause_button = false;
                    }
                    I::SetText { component, text } => {
                        let text = text_377(text);
                        if let Some(com) = self.iface_mut(component as usize) {
                            com.text = text;
                        }
                        if self.ifaces[component as usize].as_ref().is_some_and(|com| {
                            com.layer_id == self.side_icon[self.active_icon as usize]
                        }) {
                            self.redraw_side = true;
                        }
                    }
                    _ => unreachable!("opcode cohort admits only captured interface operations"),
                }
                p.iface = true;
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
