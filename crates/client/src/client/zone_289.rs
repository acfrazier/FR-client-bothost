//! R289 world operations, staged against the exact outer frame. Primary:
//! pinned client.java method149, full-follows2755 and rebuild2999.
use super::*;

pub(super) struct ZoneFrame {
    origin: Option<(i32, i32)>,
    clear: bool,
    operations: Vec<ZoneOperation>,
}

enum ZoneOperation {
    Merge {
        x: i32,
        z: i32,
        shape: i32,
        angle: i32,
        id: i32,
        t1: i32,
        t2: i32,
        pid: i32,
        east: i32,
        south: i32,
        west: i32,
        north: i32,
    },
    LocAnim {
        x: i32,
        z: i32,
        shape: i32,
        angle: i32,
        seq: i32,
    },
    Projectile {
        x: i32,
        z: i32,
        x2: i32,
        z2: i32,
        target: i32,
        id: i32,
        h1: i32,
        h2: i32,
        t1: i32,
        t2: i32,
        angle: i32,
        startpos: i32,
    },
    MapAnim {
        x: i32,
        z: i32,
        id: i32,
        height: i32,
        time: i32,
    },
    LocChange {
        x: i32,
        z: i32,
        id: i32,
        shape: i32,
        angle: i32,
    },
    ObjectAdd {
        x: i32,
        z: i32,
        id: i32,
        count: i32,
        receiver: Option<i32>,
    },
    ObjectDelete {
        x: i32,
        z: i32,
        id: i32,
    },
    ObjectCount {
        x: i32,
        z: i32,
        id: i32,
        old: i32,
        count: i32,
    },
    AreaSound {
        x: i32,
        z: i32,
        id: i32,
        radius: i32,
        loops: i32,
    },
}

impl ZoneFrame {
    pub(super) fn decode(client: &Client, id: i32, p: &mut Packet) -> Self {
        assert!((0..4).contains(&client.minusedlevel), "invalid zone plane");
        let enclosed = id == ServerProt289::UPDATE_ZONE_PARTIAL_ENCLOSED;
        let follows = matches!(id, 144 | 155);
        let origin = (enclosed || follows).then(|| (p.g1(), p.g1()));
        let (x, z) = origin.unwrap_or((client.zone_update_x, client.zone_update_z));
        let mut operations = Vec::new();
        if enclosed {
            while p.available() > 0 {
                let inner = p.g1();
                operations.push(ZoneOperation::decode(inner, x, z, p));
            }
        } else if !follows {
            operations.push(ZoneOperation::decode(id, x, z, p));
        }
        for operation in &operations {
            operation.validate(client);
        }
        Self {
            origin,
            clear: id == 144,
            operations,
        }
    }

    pub(super) fn apply(self, c: &mut Client) -> R289Publication {
        let mut publication = R289Publication::default();
        if let Some((x, z)) = self.origin {
            c.zone_update_x = x;
            c.zone_update_z = z;
            publication.scene = true;
            if self.clear {
                for tx in x..x + 8 {
                    for tz in z..z + 8 {
                        if tile(tx, tz)
                            && c.ground_obj[c.minusedlevel as usize][tx as usize][tz as usize]
                                .take()
                                .is_some()
                        {
                            c.show_object(tx, tz);
                        }
                    }
                }
                let mut node = c.loc_changes.head();
                while let Some(loc) = node {
                    if loc.level == c.minusedlevel
                        && (x..x + 8).contains(&loc.x)
                        && (z..z + 8).contains(&loc.z)
                    {
                        loc.end_time = 0;
                    }
                    node = c.loc_changes.next_node();
                }
            }
        }
        for op in self.operations {
            op.apply(c, &mut publication);
        }
        publication
    }
}

fn tile(x: i32, z: i32) -> bool {
    (0..BuildArea::SIZE).contains(&x) && (0..BuildArea::SIZE).contains(&z)
}

fn shape_angle(p: &mut Packet) -> (i32, i32) {
    let info = p.g1();
    let shape = info >> 2;
    assert!(
        (shape as usize) < LOC_SHAPE_TO_LAYER.len(),
        "invalid zone loc shape"
    );
    (shape, info & 3)
}

impl ZoneOperation {
    fn validate(&self, c: &Client) {
        match *self {
            Self::ObjectAdd { id, .. } => {
                assert!((id as usize) < c.cache.objs.len(), "invalid zone object")
            }
            Self::Merge { id, .. } => {
                assert!((id as usize) < c.cache.locs.len(), "invalid merge loc")
            }
            Self::LocChange { id, .. } if id >= 0 => {
                assert!((id as usize) < c.cache.locs.len(), "invalid zone loc")
            }
            Self::LocAnim { seq, .. } => {
                assert!((seq as usize) < c.cache.seqs.len(), "invalid loc sequence")
            }
            Self::MapAnim { id, .. } | Self::Projectile { id, .. } => {
                assert!((id as usize) < c.cache.spots.len(), "invalid zone spotanim")
            }
            Self::AreaSound { id, .. } => {
                assert!((id as usize) < c.jagfx.delays.len(), "invalid area sound")
            }
            _ => {}
        }
    }

    fn decode(id: i32, origin_x: i32, origin_z: i32, p: &mut Packet) -> Self {
        let pos = p.g1();
        let x = origin_x + ((pos >> 4) & 7);
        let z = origin_z + (pos & 7);
        match id {
            83 => {
                let (shape, angle) = shape_angle(p);
                let op = Self::Merge {
                    x,
                    z,
                    shape,
                    angle,
                    id: p.g2(),
                    t1: p.g2(),
                    t2: p.g2(),
                    pid: p.g2(),
                    east: p.g1b(),
                    south: p.g1b(),
                    west: p.g1b(),
                    north: p.g1b(),
                };
                if let Self::Merge { pid, .. } = op {
                    assert!(pid < 2048, "invalid merge player slot");
                }
                assert!(tile(x, z), "invalid merge tile");
                op
            }
            106 => {
                let (shape, angle) = shape_angle(p);
                Self::LocAnim {
                    x,
                    z,
                    shape,
                    angle,
                    seq: p.g2(),
                }
            }
            87 => Self::Projectile {
                x,
                z,
                x2: x + p.g1b(),
                z2: z + p.g1b(),
                target: p.g2b(),
                id: p.g2(),
                h1: p.g1() * 4,
                h2: p.g1() * 4,
                t1: p.g2(),
                t2: p.g2(),
                angle: p.g1(),
                startpos: p.g1(),
            },
            233 => Self::MapAnim {
                x,
                z,
                id: p.g2(),
                height: p.g1(),
                time: p.g2(),
            },
            90 | 194 => {
                let (shape, angle) = shape_angle(p);
                Self::LocChange {
                    x,
                    z,
                    shape,
                    angle,
                    id: if id == 194 { -1 } else { p.g2() },
                }
            }
            60 | 176 => Self::ObjectAdd {
                x,
                z,
                id: p.g2(),
                count: p.g2(),
                receiver: (id == 176).then(|| p.g2()),
            },
            71 => Self::ObjectDelete {
                x,
                z,
                id: p.g2() & 32767,
            },
            117 => Self::ObjectCount {
                x,
                z,
                id: p.g2() & 32767,
                old: p.g2(),
                count: p.g2(),
            },
            // Additional primary operation; not legacy LAST_LOGIN_INFO91.
            91 => {
                let id = p.g2();
                let info = p.g1();
                Self::AreaSound {
                    x,
                    z,
                    id,
                    radius: (info >> 4) & 15,
                    loops: info & 7,
                }
            }
            _ => panic!("unknown enclosed zone operation {id}"),
        }
    }

    fn apply(self, c: &mut Client, publication: &mut R289Publication) {
        match self {
            Self::Merge {
                x,
                z,
                shape,
                angle,
                id,
                t1,
                t2,
                pid,
                east,
                south,
                west,
                north,
            } => {
                publication.scene = true;
                let player = if pid == c.self_slot {
                    c.local_player.as_ref()
                } else {
                    c.players[pid as usize].as_deref()
                };
                if player.is_none() {
                    return;
                }
                let level = c.minusedlevel;
                let h = &c.groundh[level as usize];
                let loc = c.cache.loc(id as usize);
                let Some(model) = loc.get_model(
                    &c.cache,
                    shape,
                    angle,
                    h[x as usize][z as usize],
                    h[x as usize + 1][z as usize],
                    h[x as usize + 1][z as usize + 1],
                    h[x as usize][z as usize + 1],
                    -1,
                ) else {
                    return;
                };
                let (width, length) = if angle == 1 || angle == 3 {
                    (loc.length, loc.width)
                } else {
                    (loc.width, loc.length)
                };
                c.loc_change_create(
                    level,
                    x,
                    z,
                    LOC_SHAPE_TO_LAYER[shape as usize],
                    -1,
                    0,
                    0,
                    t1 + 1,
                    t2 + 1,
                );
                let player = if pid == c.self_slot {
                    c.local_player.as_mut()
                } else {
                    c.players[pid as usize].as_deref_mut()
                }
                .unwrap();
                player.loc_start_cycle = t1 + c.loop_cycle;
                player.loc_stop_cycle = t2 + c.loop_cycle;
                player.loc_model = Some(Box::new(model));
                player.loc_offset_x = x * 128 + width * 64;
                player.loc_offset_z = z * 128 + length * 64;
                player.loc_offset_y = get_av_h(
                    &c.groundh,
                    &c.mapl,
                    player.loc_offset_x,
                    player.loc_offset_z,
                    level,
                );
                player.min_tile_x = x + east.min(west);
                player.max_tile_x = x + east.max(west);
                player.min_tile_z = z + south.min(north);
                player.max_tile_z = z + south.max(north);
                publication.player = true;
            }
            Self::LocAnim {
                x,
                z,
                shape,
                angle,
                seq,
            } => {
                publication.scene = true;
                if !(0..103).contains(&x) || !(0..103).contains(&z) {
                    return;
                }
                let level = c.minusedlevel;
                let h = &c.groundh[level as usize];
                let (sw, se, ne, nw) = (
                    h[x as usize][z as usize],
                    h[x as usize + 1][z as usize],
                    h[x as usize + 1][z as usize + 1],
                    h[x as usize][z as usize + 1],
                );
                match LOC_SHAPE_TO_LAYER[shape as usize] {
                    LocLayer::WALL => {
                        if let Some(w) = c.world.get_wall_mut(level, x, z) {
                            w.anim_seq = seq;
                            w.anim_shape = shape;
                            w.anim_angle = angle;
                            w.h_sw = sw;
                            w.h_se = se;
                            w.h_ne = ne;
                            w.h_nw = nw;
                            c.world.bump_loc_stamp(level, x, z, LocLayer::WALL);
                        }
                    }
                    LocLayer::WALL_DECOR => {
                        if let Some(w) = c.world.get_decor_mut(level, x, z) {
                            w.anim_seq = seq;
                            w.anim_shape = 4;
                            w.anim_angle = 0;
                            w.h_sw = sw;
                            w.h_se = se;
                            w.h_ne = ne;
                            w.h_nw = nw;
                            c.world.bump_loc_stamp(level, x, z, LocLayer::WALL_DECOR);
                        }
                    }
                    LocLayer::GROUND => {
                        if let Some(w) = c.world.get_scene_mut(level, x, z) {
                            w.anim_seq = seq;
                            w.anim_shape = if shape == 11 { 10 } else { shape };
                            w.anim_angle = angle;
                            w.h_sw = sw;
                            w.h_se = se;
                            w.h_ne = ne;
                            w.h_nw = nw;
                            w.model_stamp = w.model_stamp.wrapping_add(1);
                        }
                    }
                    LocLayer::GROUND_DECOR => {
                        if let Some(w) = c.world.get_gd_mut(level, x, z) {
                            w.anim_seq = seq;
                            w.anim_shape = 22;
                            w.anim_angle = angle;
                            w.h_sw = sw;
                            w.h_se = se;
                            w.h_ne = ne;
                            w.h_nw = nw;
                            c.world.bump_loc_stamp(level, x, z, LocLayer::GROUND_DECOR);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Self::Projectile {
                x,
                z,
                x2,
                z2,
                target,
                id,
                h1,
                h2,
                t1,
                t2,
                angle,
                startpos,
            } => {
                publication.scene = true;
                if !tile(x, z) || !tile(x2, z2) {
                    return;
                }
                let (x, z, x2, z2) = (x * 128 + 64, z * 128 + 64, x2 * 128 + 64, z2 * 128 + 64);
                let mut proj = ClientProj::new(
                    id,
                    c.minusedlevel,
                    x,
                    get_av_h(&c.groundh, &c.mapl, x, z, c.minusedlevel) - h1,
                    z,
                    t1 + c.loop_cycle,
                    t2 + c.loop_cycle,
                    angle,
                    startpos,
                    target,
                    h2,
                );
                proj.set_target(
                    x2 as f64,
                    (get_av_h(&c.groundh, &c.mapl, x2, z2, c.minusedlevel) - h2) as f64,
                    z2 as f64,
                    t1 + c.loop_cycle,
                );
                proj.bind_seq(&c.cache);
                c.projectiles.push(proj);
            }
            Self::MapAnim {
                x,
                z,
                id,
                height,
                time,
            } => {
                publication.scene = true;
                if !tile(x, z) {
                    return;
                }
                let (x, z) = (x * 128 + 64, z * 128 + 64);
                c.spotanims.push(MapSpotAnim::new(
                    id,
                    c.minusedlevel,
                    x,
                    z,
                    get_av_h(&c.groundh, &c.mapl, x, z, c.minusedlevel) - height,
                    c.loop_cycle,
                    time,
                ));
            }
            Self::LocChange {
                x,
                z,
                id,
                shape,
                angle,
            } => {
                publication.scene = true;
                if tile(x, z) {
                    c.loc_change_create(
                        c.minusedlevel,
                        x,
                        z,
                        LOC_SHAPE_TO_LAYER[shape as usize],
                        id,
                        shape,
                        angle,
                        0,
                        -1,
                    );
                }
            }
            Self::ObjectAdd {
                x,
                z,
                id,
                count,
                receiver,
            } => {
                publication.scene = true;
                if tile(x, z) && receiver != Some(c.self_slot) {
                    c.ground_obj[c.minusedlevel as usize][x as usize][z as usize]
                        .get_or_insert_with(|| Box::new(LinkList::new()))
                        .push(ClientObj::new(id, count));
                    c.show_object(x, z);
                }
            }
            Self::ObjectDelete { x, z, id } => {
                publication.scene = true;
                if !tile(x, z) {
                    return;
                }
                let cell = &mut c.ground_obj[c.minusedlevel as usize][x as usize][z as usize];
                if let Some(objs) = cell {
                    let mut node = objs.head();
                    while let Some(obj) = node {
                        if obj.id == id {
                            objs.unlink_last();
                            break;
                        }
                        node = objs.next_node();
                    }
                    if objs.head().is_none() {
                        *cell = None;
                    }
                    c.show_object(x, z);
                }
            }
            Self::ObjectCount {
                x,
                z,
                id,
                old,
                count,
            } => {
                publication.scene = true;
                if !tile(x, z) {
                    return;
                }
                if let Some(objs) =
                    &mut c.ground_obj[c.minusedlevel as usize][x as usize][z as usize]
                {
                    let mut node = objs.head();
                    while let Some(obj) = node {
                        if obj.id == id && obj.count == old {
                            obj.count = count;
                            break;
                        }
                        node = objs.next_node();
                    }
                    c.show_object(x, z);
                }
            }
            Self::AreaSound {
                x,
                z,
                id,
                radius,
                loops,
            } => {
                let in_range = c.local_player.as_ref().is_some_and(|p| {
                    (x - radius..=x + radius).contains(&p.route_x[0])
                        && (z - radius..=z + radius).contains(&p.route_z[0])
                });
                if in_range && c.wave_enabled && !c.config.lowmem && (0..50).contains(&c.wave_count)
                {
                    let index = c.wave_count as usize;
                    c.wave_ids[index] = id;
                    c.wave_loops[index] = loops;
                    c.wave_delay[index] = c.jagfx.delays.get(id as usize).copied().unwrap_or(0);
                    c.wave_count += 1;
                }
            }
        }
    }
}
