//! Section G. Public primary client.java anchors are recorded per operation.
use super::{get_av_h, Client, Packet, R289Publication};

pub(super) enum MiscOperation {
    Camera {
        look: bool,
        x: i32,
        z: i32,
        height: i32,
        rate: i32,
        rate2: i32,
        y: i32,
    },
    Shake {
        axis: usize,
        jitter: i32,
        amplitude: i32,
        frequency: i32,
    },
    CameraReset,
    Minimap(i32),
    MapFlagClear,
    Multiway(i32),
    Reboot(i32),
    Song(i32),
    Jingle {
        id: i32,
        delay: i32,
    },
    Synth {
        id: i32,
        loops: i32,
        delay: Option<i32>,
    },
    Hint {
        kind: i32,
        first: i32,
        second: i32,
        height: i32,
    },
}

impl MiscOperation {
    pub(super) fn decode(c: &Client, id: i32, p: &mut Packet) -> Option<Self> {
        match id {
            // J:2734-2738,2912-2916,3154-3158,3466-3470.
            136 => Some(Self::Minimap(p.g1())),
            164 => Some(Self::MapFlagClear),
            247 => Some(Self::Multiway(p.g1())),
            204 => Some(Self::Reboot(p.g2() * 30)),
            // J:3302-3340. Only song187 converts the sentinel.
            187 => {
                let song = p.g2();
                Some(Self::Song(if song == 65535 { -1 } else { song }))
            }
            29 => Some(Self::Jingle {
                id: p.g2(),
                delay: p.g2(),
            }),
            177 => {
                let (id, loops, delay) = (p.g2(), p.g1(), p.g2());
                let delay = if c.wave_enabled && !c.config.lowmem && c.wave_count < 50 {
                    let slot = usize::try_from(c.wave_count).expect("negative sound count");
                    assert!(
                        slot < c.wave_ids.len()
                            && slot < c.wave_loops.len()
                            && slot < c.wave_delay.len(),
                        "sound queue index"
                    );
                    let base = c.jagfx.delays.get(id as usize).expect("sound config index");
                    Some(delay.checked_add(*base).expect("sound delay overflow"))
                } else {
                    None
                };
                Some(Self::Synth { id, loops, delay })
            }
            // J:2804-2818 and 3417-3443. Resolve terrain before mutation.
            73 | 82 => {
                let (x, z, height, rate, rate2) = (p.g1(), p.g1(), p.g2(), p.g1(), p.g1());
                assert!((0..4).contains(&c.minusedlevel), "camera plane");
                let y = get_av_h(
                    &c.groundh,
                    &c.mapl,
                    x * 128 + 64,
                    z * 128 + 64,
                    c.minusedlevel,
                ) - height;
                Some(Self::Camera {
                    look: id == 82,
                    x,
                    z,
                    height,
                    rate,
                    rate2,
                    y,
                })
            }
            // J:2959-2971. Validate all parallel arrays before any write.
            208 => {
                let (axis, jitter, amplitude, frequency) =
                    (p.g1() as usize, p.g1(), p.g1(), p.g1());
                assert!(
                    axis < 5
                        && axis < c.cam_shake.len()
                        && axis < c.cam_shake_axis.len()
                        && axis < c.cam_shake_ran.len()
                        && axis < c.cam_shake_amp.len()
                        && axis < c.cam_shake_cycle.len(),
                    "camera shake axis"
                );
                Some(Self::Shake {
                    axis,
                    jitter,
                    amplitude,
                    frequency,
                })
            }
            133 => {
                assert!(c.cam_shake.len() >= 5, "camera reset axes");
                Some(Self::CameraReset)
            }
            // J:2684-2720, Class17 fixed length six. Ignore padding values.
            115 => {
                let kind = p.g1();
                let (first, second, height) = match kind {
                    1 | 10 => {
                        let slot = p.g2();
                        for _ in 0..3 {
                            p.g1();
                        }
                        let capacity = if kind == 1 {
                            c.npc.len()
                        } else {
                            c.players.len()
                        };
                        assert!((slot as usize) < capacity, "hint actor index");
                        (slot, 0, 0)
                    }
                    2..=6 => (p.g2(), p.g2(), p.g1()),
                    _ => {
                        for _ in 0..5 {
                            p.g1();
                        }
                        (0, 0, 0)
                    }
                };
                Some(Self::Hint {
                    kind,
                    first,
                    second,
                    height,
                })
            }
            _ => None,
        }
    }

    pub(super) fn apply(self, c: &mut Client) -> R289Publication {
        let mut publication = R289Publication::default();
        match self {
            Self::Minimap(state) => c.minimap_state = state,
            Self::MapFlagClear => {
                c.minimap_flag_x = 0;
                publication.map_flag = true;
            }
            Self::Multiway(value) => {
                c.in_multizone = value;
                publication.world = true;
            }
            Self::Reboot(timer) => c.reboot_timer = timer,
            Self::Song(id) => {
                if c.next_midi_song != id
                    && c.midi_active
                    && !c.config.lowmem
                    && c.next_music_delay == 0
                {
                    c.midi_song = id;
                    c.midi_fading = true;
                    if let Some(od) = &mut c.on_demand {
                        od.request(2, id);
                    }
                }
                c.next_midi_song = id;
            }
            Self::Jingle { id, delay } => {
                if c.midi_active && !c.config.lowmem {
                    c.midi_song = id;
                    c.midi_fading = false;
                    if let Some(od) = &mut c.on_demand {
                        od.request(2, id);
                    }
                    c.next_music_delay = delay;
                }
            }
            Self::Synth { id, loops, delay } => {
                if let Some(delay) = delay {
                    let slot = c.wave_count as usize;
                    c.wave_ids[slot] = id;
                    c.wave_loops[slot] = loops;
                    c.wave_delay[slot] = delay;
                    c.wave_count += 1;
                }
            }
            Self::Camera {
                look,
                x,
                z,
                height,
                rate,
                rate2,
                y,
            } => {
                c.cinema_cam = true;
                if look {
                    c.cam_look_at_lx = x;
                    c.cam_look_at_lz = z;
                    c.cam_look_at_hei = height;
                    c.cam_look_at_rate = rate;
                    c.cam_look_at_rate2 = rate2;
                    if rate2 >= 100 {
                        let dx = (x * 128 + 64) as f64 - c.cam_x as f64;
                        let dz = (z * 128 + 64) as f64 - c.cam_z as f64;
                        let dy = y as f64 - c.cam_y as f64;
                        let distance = (dx * dx + dz * dz).sqrt() as i32;
                        c.cam_pitch =
                            ((dy.atan2(distance as f64) * 325.949) as i32 & 0x7ff).clamp(128, 383);
                        c.cam_yaw = (dx.atan2(dz) * -325.949) as i32 & 0x7ff;
                    }
                } else {
                    c.cam_move_to_lx = x;
                    c.cam_move_to_lz = z;
                    c.cam_move_to_hei = height;
                    c.cam_move_to_rate = rate;
                    c.cam_move_to_rate2 = rate2;
                    if rate2 >= 100 {
                        (c.cam_x, c.cam_y, c.cam_z) = (x * 128 + 64, y, z * 128 + 64);
                    }
                }
                publication.camera = true;
            }
            Self::Shake {
                axis,
                jitter,
                amplitude,
                frequency,
            } => {
                c.cam_shake[axis] = true;
                c.cam_shake_axis[axis] = jitter;
                c.cam_shake_ran[axis] = amplitude;
                c.cam_shake_amp[axis] = frequency;
                c.cam_shake_cycle[axis] = 0;
                publication.camera = true;
            }
            Self::CameraReset => {
                c.cinema_cam = false;
                c.cam_shake[..5].fill(false);
                publication.camera = true;
            }
            Self::Hint {
                kind,
                first,
                second,
                height,
            } => {
                c.hint_type = kind;
                match kind {
                    1 => c.hint_npc = first,
                    10 => c.hint_player = first,
                    2..=6 => {
                        let offsets = [(64, 64), (0, 64), (128, 64), (64, 0), (64, 128)];
                        (c.hint_offset_x, c.hint_offset_z) = offsets[(kind - 2) as usize];
                        c.hint_type = 2;
                        c.hint_tile_x = first;
                        c.hint_tile_z = second;
                        c.hint_height = height;
                    }
                    _ => {}
                }
            }
        }
        publication
    }
}
