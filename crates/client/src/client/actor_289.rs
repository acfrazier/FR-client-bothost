//! Internal R289 actor transaction. Source: pinned client.java methods
//! 139/212/185/172/153/128 and 187/226/124/222 (see cleanup-d-report).
//! Decode borrows Client immutably; apply never reads a packet. Only wire
//! operations/appearance values are staged, never actors, worlds or models.
use super::{Client, ClientNpc, ClientPlayer, R289Publication, LOCAL_PLAYER_INDEX};
use crate::config::Cache;
use crate::dash3d::client_entity::ClientEntity;
use crate::dash3d::client_player::recol1d;
use crate::io::Packet;
use crate::util::JString;
use crate::wordfilter::{WordFilter, WordPack};

pub(super) struct ActorFrame {
    npc: bool,
    plane: Option<i32>,
    retained: Vec<i32>,
    removed: Vec<i32>,
    movements: Vec<(usize, Movement)>,
    masks: Vec<(usize, Vec<Mask>)>,
}

enum Movement {
    Step {
        run: bool,
        first: i32,
        second: Option<i32>,
    },
    Teleport {
        x: i32,
        z: i32,
        jump: bool,
    },
    New {
        dx: i32,
        dz: i32,
        jump: bool,
        npc_type: Option<usize>,
        appearance: Option<Appearance>,
    },
}

enum Mask {
    Appearance(Appearance),
    Anim(i32, i32),
    Face(i32),
    Say(String),
    Hit {
        damage: i32,
        kind: i32,
        health: i32,
        max: i32,
    },
    Square(i32, i32),
    Chat {
        colour_effect: i32,
        staff: i32,
        text: String,
    },
    Spot(i32, i32),
    Exact {
        x0: i32,
        z0: i32,
        x1: i32,
        z1: i32,
        end: i32,
        start: i32,
        facing: i32,
    },
    Type(usize),
}

struct Appearance {
    raw: Vec<u8>,
    gender: i32,
    icons: i32,
    parts: Vec<u16>,
    transmog: Option<usize>,
    colours: [u16; 5],
    anims: [i32; 7],
    name: String,
    combat: i32,
    skill: i32,
}

fn optional_id(id: i32) -> i32 {
    if id == 65535 {
        -1
    } else {
        id
    }
}

/// gbit's legacy backing-allocation behavior stays untouched for R274.
fn bit(p: &mut Packet, width: usize) -> i32 {
    assert!(
        p.bit_pos + width <= p.frame_end().expect("bounded actor frame") * 8,
        "truncated actor bits"
    );
    p.gbit(width)
}
fn signed5(p: &mut Packet) -> i32 {
    let n = bit(p, 5);
    if n > 15 {
        n - 32
    } else {
        n
    }
}
fn bytes(p: &mut Packet, length: usize) -> Vec<u8> {
    assert!(length <= p.available(), "truncated actor bytes");
    let mut data = vec![0; length];
    p.gdata(length, 0, &mut data);
    data
}

impl Appearance {
    // Class1_Sub1_Sub1_Sub1_Sub1.method39: transmog stops the part loop,
    // retaining the unwritten slots of an existing actor's appearance.
    fn decode(raw: Vec<u8>) -> Self {
        let mut p = Packet::new(raw.clone());
        p.set_frame_end(raw.len());
        let gender = p.g1();
        let icons = p.g1();
        let mut parts = Vec::new();
        let mut transmog = None;
        for part in 0..12 {
            let high = p.g1();
            let value = if high == 0 { 0 } else { (high << 8) | p.g1() };
            parts.push(value as u16);
            if part == 0 && value == 65535 {
                transmog = Some(p.g2() as usize);
                break;
            }
        }
        let mut colours = [0; 5];
        for (i, colour) in colours.iter_mut().enumerate() {
            let value = p.g1() as usize;
            *colour = if value < recol1d()[i].len() {
                value as u16
            } else {
                0
            };
        }
        let mut anims = [0; 7];
        for anim in &mut anims {
            *anim = optional_id(p.g2());
        }
        let name = JString::to_screen_name(&JString::to_raw_username(p.g8()));
        let combat = p.g1();
        let skill = p.g2();
        assert_eq!(p.available(), 0, "unconsumed appearance");
        Self {
            raw,
            gender,
            icons,
            parts,
            transmog,
            colours,
            anims,
            name,
            combat,
            skill,
        }
    }

    fn apply(self, player: &mut ClientPlayer, cache: &Cache) -> Packet {
        player.gender = self.gender;
        player.headicons = self.icons;
        player.appearance[..self.parts.len()].copy_from_slice(&self.parts);
        player.transmog = self.transmog.filter(|&id| id < cache.npcs.len());
        player.colour = self.colours;
        // 289 `method39` 128-155 reads the side-steps in the Java 274
        // `setAppearance` order: fifth `walkanim_r`, sixth `walkanim_l`.
        [
            player.readyanim,
            player.turnanim,
            player.walkanim,
            player.walkanim_b,
            player.walkanim_r,
            player.walkanim_l,
            player.runanim,
        ] = self.anims;
        player.name = Some(self.name);
        player.combat_level = self.combat;
        player.skill_level = self.skill;
        player.ready = true;
        let mut base = 0i64;
        for part in player.appearance {
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
        Packet::new(self.raw)
    }
}

impl ActorFrame {
    pub(super) fn decode(c: &Client, npc: bool, p: &mut Packet) -> Self {
        let mut frame = Self {
            npc,
            plane: None,
            retained: Vec::new(),
            removed: Vec::new(),
            movements: Vec::new(),
            masks: Vec::new(),
        };
        let mut updates = Vec::new();
        p.gbit_start();
        if !npc && bit(p, 1) != 0 {
            let local = LOCAL_PLAYER_INDEX as usize;
            assert!(c.local_player.is_some(), "missing local actor");
            match bit(p, 2) {
                0 => updates.push(local),
                kind @ (1 | 2) => {
                    let first = bit(p, 3);
                    let second = if kind == 2 { Some(bit(p, 3)) } else { None };
                    frame.movements.push((
                        local,
                        Movement::Step {
                            run: kind == 2,
                            first,
                            second,
                        },
                    ));
                    if bit(p, 1) != 0 {
                        updates.push(local);
                    }
                }
                3 => {
                    frame.plane = Some(bit(p, 2));
                    let x = bit(p, 7);
                    let z = bit(p, 7);
                    let jump = bit(p, 1) != 0;
                    frame
                        .movements
                        .push((local, Movement::Teleport { x, z, jump }));
                    if bit(p, 1) != 0 {
                        updates.push(local);
                    }
                }
                _ => unreachable!(),
            }
        }
        let (old_count, ids) = if npc {
            (c.npc_count, &c.npc_ids)
        } else {
            (c.player_count, &c.player_ids)
        };
        let old_count = usize::try_from(old_count).expect("negative actor count");
        assert!(old_count <= ids.len());
        let count = bit(p, 8) as usize;
        assert!(count <= old_count, "too many retained actors");
        frame.removed.extend_from_slice(&ids[count..old_count]);
        for &id in &ids[..count] {
            let index = usize::try_from(id).expect("negative actor index");
            let exists = if npc {
                c.npc.get(index).is_some_and(Option::is_some)
            } else {
                c.players.get(index).is_some_and(Option::is_some)
            };
            assert!(exists, "missing retained actor");
            if bit(p, 1) == 0 {
                frame.retained.push(id);
                continue;
            }
            match bit(p, 2) {
                0 => {
                    frame.retained.push(id);
                    updates.push(index);
                }
                kind @ (1 | 2) => {
                    frame.retained.push(id);
                    let first = bit(p, 3);
                    let second = if kind == 2 { Some(bit(p, 3)) } else { None };
                    frame.movements.push((
                        index,
                        Movement::Step {
                            run: kind == 2,
                            first,
                            second,
                        },
                    ));
                    if bit(p, 1) != 0 {
                        updates.push(index);
                    }
                }
                3 => frame.removed.push(id),
                _ => unreachable!(),
            }
        }
        // Exact Java loop thresholds, including no-new-record endings. Do
        // not reinterpret potential masks as new records using mask heuristics.
        let threshold = if npc { 21 } else { 10 };
        while p.bit_pos + threshold < p.frame_end().unwrap() * 8 {
            let id = bit(p, if npc { 14 } else { 11 });
            if id == if npc { 16383 } else { 2047 } {
                break;
            }
            let index = id as usize;
            assert!(index < if npc { c.npc.len() } else { c.players.len() });
            let npc_type = if npc { Some(bit(p, 11) as usize) } else { None };
            let dx = signed5(p);
            let dz = signed5(p);
            let jump = bit(p, 1) != 0;
            let extended = bit(p, 1) != 0;
            let appearance = if !npc && c.players[index].is_none() {
                c.player_appearance_buffer[index]
                    .as_ref()
                    .map(|p| Appearance::decode(p.data().to_vec()))
            } else {
                None
            };
            assert!(c.local_player.is_some(), "missing local origin");
            frame.retained.push(id);
            frame.movements.push((
                index,
                Movement::New {
                    dx,
                    dz,
                    jump,
                    npc_type,
                    appearance,
                },
            ));
            if extended {
                updates.push(index);
            }
        }
        p.gbit_end();
        assert!(frame.retained.len() <= ids.len());
        assert!(frame.removed.len() <= c.entity_removal_ids.len());
        assert!(updates.len() <= c.entity_update_ids.len());
        for &id in &frame.removed {
            assert!(id >= 0 && (id as usize) < if npc { c.npc.len() } else { c.players.len() });
        }
        for index in updates {
            let mut mask = p.g1();
            if !npc && mask & 128 != 0 {
                mask |= p.g1() << 8;
            }
            let mut operations = Vec::new();
            // Primary methods128/222 use ascending bit order, NOT the
            // legacy symbolic HITMARK/HITMARK2 names (which are reversed).
            if mask & 1 != 0 {
                if npc {
                    operations.push(Self::hit(p));
                } else {
                    let length = p.g1() as usize;
                    operations.push(Mask::Appearance(Appearance::decode(bytes(p, length))));
                }
            }
            if mask & 2 != 0 {
                operations.push(Mask::Anim(optional_id(p.g2()), p.g1()));
            }
            if mask & 4 != 0 {
                operations.push(Mask::Face(optional_id(p.g2())));
            }
            if mask & 8 != 0 {
                operations.push(Mask::Say(p.gjstr()));
            }
            if mask & 16 != 0 {
                operations.push(Self::hit(p));
            }
            if mask & 32 != 0 {
                operations.push(if npc {
                    Mask::Type(p.g2() as usize)
                } else {
                    Mask::Square(p.g2(), p.g2())
                });
            }
            if mask & 64 != 0 {
                if npc {
                    operations.push(Mask::Spot(optional_id(p.g2()), p.g4()));
                } else {
                    let colour_effect = p.g2();
                    let staff = p.g1();
                    let length = p.g1() as usize;
                    let mut packed = Packet::new(bytes(p, length));
                    let text = WordPack::unpack(&mut packed, length);
                    operations.push(Mask::Chat {
                        colour_effect,
                        staff,
                        text,
                    });
                }
            }
            if npc {
                if mask & 128 != 0 {
                    operations.push(Mask::Square(p.g2(), p.g2()));
                }
            } else {
                if mask & 256 != 0 {
                    operations.push(Mask::Spot(optional_id(p.g2()), p.g4()));
                }
                if mask & 512 != 0 {
                    operations.push(Mask::Exact {
                        x0: p.g1(),
                        z0: p.g1(),
                        x1: p.g1(),
                        z1: p.g1(),
                        end: p.g2(),
                        start: p.g2(),
                        facing: p.g1(),
                    });
                }
                if mask & 1024 != 0 {
                    operations.push(Self::hit(p));
                }
            }
            frame.masks.push((index, operations));
        }
        assert_eq!(p.available(), 0, "unconsumed actor frame");
        frame
    }

    fn hit(p: &mut Packet) -> Mask {
        Mask::Hit {
            damage: p.g1(),
            kind: p.g1(),
            health: p.g1(),
            max: p.g1(),
        }
    }

    pub(super) fn apply(self, c: &mut Client) -> R289Publication {
        let mut publication = R289Publication {
            player: !self.npc,
            npc: self.npc,
            player_info: !self.npc,
            ..Default::default()
        };
        if let Some(plane) = self.plane {
            c.minusedlevel = plane;
        }
        for (index, movement) in self.movements {
            match movement {
                Movement::New {
                    dx,
                    dz,
                    jump,
                    npc_type,
                    appearance,
                } => {
                    let local = c.local_player.as_ref().unwrap();
                    let (x, z) = (local.route_x[0] + dx, local.route_z[0] + dz);
                    if self.npc {
                        let npc =
                            c.npc[index].get_or_insert_with(|| Box::new(ClientNpc::default()));
                        bind_type(npc, npc_type.unwrap(), &c.cache);
                        npc.teleport(&c.cache, jump, x, z);
                    } else {
                        let player = c.players[index]
                            .get_or_insert_with(|| Box::new(ClientPlayer::default()));
                        if let Some(appearance) = appearance {
                            c.player_appearance_buffer[index] =
                                Some(Box::new(appearance.apply(player, &c.cache)));
                        }
                        player.teleport(&c.cache, jump, x, z);
                    }
                }
                Movement::Step { run, first, second } => {
                    let cache = c.cache.clone();
                    let entity = actor(c, self.npc, index);
                    entity.move_code(&cache, run, first);
                    if let Some(second) = second {
                        entity.move_code(&cache, run, second);
                    }
                }
                Movement::Teleport { x, z, jump } => {
                    let cache = c.cache.clone();
                    actor(c, self.npc, index).teleport(&cache, jump, x, z);
                }
            }
        }
        if self.npc {
            c.npc_count = self.retained.len() as i32;
            c.npc_ids[..self.retained.len()].copy_from_slice(&self.retained);
        } else {
            c.player_count = self.retained.len() as i32;
            c.player_ids[..self.retained.len()].copy_from_slice(&self.retained);
            c.awaiting_player_info = false;
        }
        let cycle = c.loop_cycle;
        for &id in &self.retained {
            actor(c, self.npc, id as usize).cycle = cycle;
        }
        c.entity_removal_count = self.removed.len() as i32;
        c.entity_removal_ids[..self.removed.len()].copy_from_slice(&self.removed);
        c.entity_update_count = self.masks.len() as i32;
        for (i, (index, masks)) in self.masks.into_iter().enumerate() {
            c.entity_update_ids[i] = index as i32;
            for mask in masks {
                publication.chat |= apply_mask(c, self.npc, index, mask);
            }
        }
        for id in self.removed {
            let id = id as usize;
            if self.npc {
                if c.npc[id].as_ref().is_some_and(|n| n.cycle != cycle) {
                    c.npc[id] = None;
                }
            } else if c.players[id].as_ref().is_some_and(|p| p.cycle != cycle) {
                c.players[id] = None;
            }
        }
        publication
    }
}

fn actor(c: &mut Client, npc: bool, index: usize) -> &mut ClientEntity {
    if npc {
        &mut c.npc[index].as_mut().unwrap().entity
    } else if index == LOCAL_PLAYER_INDEX as usize {
        &mut c.local_player.as_mut().unwrap().entity
    } else {
        &mut c.players[index].as_mut().unwrap().entity
    }
}
fn player(c: &mut Client, index: usize) -> &mut ClientPlayer {
    if index == LOCAL_PLAYER_INDEX as usize {
        c.local_player.as_mut().unwrap()
    } else {
        c.players[index].as_deref_mut().unwrap()
    }
}
fn bind_type(npc: &mut ClientNpc, id: usize, cache: &Cache) {
    if id < cache.npcs.len() {
        let kind = cache.npc(id);
        npc.r#type = Some(id);
        npc.size = kind.size;
        npc.turnspeed = kind.turnspeed;
        npc.walkanim = kind.walkanim;
        npc.walkanim_b = kind.walkanim_b;
        npc.walkanim_l = kind.walkanim_l;
        npc.walkanim_r = kind.walkanim_r;
        npc.readyanim = kind.readyanim;
    } else {
        npc.r#type = None;
    }
}
fn animate(entity: &mut ClientEntity, id: i32, delay: i32, cache: &Cache) {
    if entity.primary_anim == id {
        entity.primary_anim_loop = 0;
    }
    if entity.primary_anim == id && id != -1 {
        if (id as usize) < cache.seqs.len() && cache.seq(id as usize).duplicatebehaviour == 1 {
            entity.primary_anim_frame = 0;
            entity.primary_anim_cycle = 0;
            entity.primary_anim_delay = delay;
            entity.primary_anim_loop = 0;
        }
    } else if id == -1
        || entity.primary_anim == -1
        || (id as usize) >= cache.seqs.len()
        || (entity.primary_anim as usize) >= cache.seqs.len()
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

fn apply_mask(c: &mut Client, npc: bool, index: usize, mask: Mask) -> bool {
    let cycle = c.loop_cycle;
    match mask {
        Mask::Appearance(appearance) => {
            let cache = c.cache.clone();
            let raw = appearance.apply(player(c, index), &cache);
            c.player_appearance_buffer[index] = Some(Box::new(raw));
        }
        Mask::Anim(id, delay) => {
            let cache = c.cache.clone();
            animate(actor(c, npc, index), id, delay, &cache);
        }
        Mask::Face(id) => actor(c, npc, index).face_entity = id,
        Mask::Say(message) => {
            let log = !npc && (message.starts_with('~') || index == LOCAL_PLAYER_INDEX as usize);
            let message = if !npc {
                message.strip_prefix('~').unwrap_or(&message).to_owned()
            } else {
                message
            };
            let entity = actor(c, npc, index);
            entity.chat_message = Some(message.clone());
            entity.chat_timer = if npc { 100 } else { 150 };
            if !npc {
                entity.chat_colour = 0;
                entity.chat_effect = 0;
            }
            if log {
                let name = player(c, index).name.clone().unwrap_or_default();
                c.add_chat(2, &message, &name);
                return true;
            }
        }
        Mask::Hit {
            damage,
            kind,
            health,
            max,
        } => {
            let entity = actor(c, npc, index);
            entity.add_hitmark(cycle, kind, damage);
            entity.combat_cycle = cycle + 300;
            entity.health = health;
            entity.total_health = max;
        }
        Mask::Square(x, z) => {
            let entity = actor(c, npc, index);
            entity.face_square_x = x;
            entity.face_square_z = z;
        }
        Mask::Chat {
            colour_effect,
            staff,
            text,
        } => {
            let p = player(c, index);
            let (name, ready) = (p.name.clone(), p.ready);
            if let (Some(name), true) = (name, ready) {
                let hash = JString::to_userhash(&name) as i64;
                let ignored = staff <= 1
                    && c.ignore_userhash
                        .iter()
                        .take(c.ignore_count.max(0) as usize)
                        .any(|&h| h == hash);
                if !ignored && c.chat_disabled == 0 {
                    let text = WordFilter::filter(&text);
                    let p = player(c, index);
                    p.chat_message = Some(text.clone());
                    p.chat_colour = colour_effect >> 8;
                    p.chat_effect = colour_effect & 255;
                    p.chat_timer = 150;
                    let (kind, name) = match staff {
                        2 | 3 => (1, format!("@cr2@{name}")),
                        1 => (1, format!("@cr1@{name}")),
                        _ => (2, name),
                    };
                    c.add_chat(kind, &text, &name);
                    return true;
                }
            }
        }
        Mask::Spot(id, info) => {
            let entity = actor(c, npc, index);
            entity.spotanim_id = id;
            entity.spotanim_height = info >> 16;
            entity.spotanim_last_cycle = cycle + (info & 65535);
            entity.spotanim_frame = if entity.spotanim_last_cycle > cycle {
                -1
            } else {
                0
            };
            entity.spotanim_cycle = 0;
        }
        Mask::Exact {
            x0,
            z0,
            x1,
            z1,
            end,
            start,
            facing,
        } => {
            let entity = actor(c, npc, index);
            entity.exact_start_x = x0;
            entity.exact_start_z = z0;
            entity.exact_end_x = x1;
            entity.exact_end_z = z1;
            entity.exact_move_end = cycle + end;
            entity.exact_move_start = cycle + start;
            entity.exact_move_facing = facing;
            entity.abort_route();
        }
        Mask::Type(id) => bind_type(c.npc[index].as_mut().unwrap(), id, &c.cache),
    }
    false
}
