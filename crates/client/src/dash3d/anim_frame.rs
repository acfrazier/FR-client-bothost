// Port of `~/experiments/Server/webclient/src/dash3d/AnimFrame.ts`. The TS
// statics `list`/`opaque` live in a process-wide store; each frame owns a
// copy of its `AnimBase` (the TS shares one base object, so contents match).
// `get` returns an owned clone so the store lock never leaks into callers.
use std::sync::{Mutex, OnceLock};

use crate::dash3d::anim_base::AnimBase;
use crate::io::Packet;

#[derive(Clone)]
pub struct AnimFrame {
    pub delay: i32,
    pub base: Option<AnimBase>,
    pub size: i32,
    pub ti: Option<Vec<i32>>,
    pub tx: Option<Vec<i32>>,
    pub ty: Option<Vec<i32>>,
    pub tz: Option<Vec<i32>>,
}

pub struct AnimFrameStore {
    pub list: Vec<Option<AnimFrame>>,
    pub opaque: Vec<bool>,
}

static STORE: OnceLock<Mutex<AnimFrameStore>> = OnceLock::new();

fn store() -> &'static Mutex<AnimFrameStore> {
    STORE.get_or_init(|| {
        Mutex::new(AnimFrameStore { list: Vec::new(), opaque: Vec::new() })
    })
}

impl AnimFrame {
    /// `AnimFrame.init(total)` from client-ts.
    pub fn init(total: i32) {
        let mut s = store().lock().unwrap();
        s.list = Vec::with_capacity(total as usize + 1);
        s.opaque = Vec::with_capacity(total as usize + 1);
        for _ in 0..=total {
            s.list.push(None);
            s.opaque.push(true);
        }
    }

    /// `AnimFrame.unpack(data)` from client-ts; `data` is one OnDemand
    /// "data" archive entry (the trailer holds the section lengths).
    pub fn unpack(data: &[u8]) {
        let buf_len = data.len();
        let mut buf = Packet::new(data.to_vec());
        buf.pos = buf_len - 8;

        let head_length = buf.g2();
        let tran1_length = buf.g2();
        let tran2_length = buf.g2();
        let del_length = buf.g2();

        let mut pos = 0usize;
        let mut head = Packet::new(data.to_vec());
        head.pos = pos;
        pos += head_length as usize + 2;

        let mut tran1 = Packet::new(data.to_vec());
        tran1.pos = pos;
        pos += tran1_length as usize;

        let mut tran2 = Packet::new(data.to_vec());
        tran2.pos = pos;
        pos += tran2_length as usize;

        let mut del = Packet::new(data.to_vec());
        del.pos = pos;
        pos += del_length as usize;

        let mut base_buf = Packet::new(data.to_vec());
        base_buf.pos = pos;
        let base = AnimBase::new(&mut base_buf);

        let total = head.g2();
        let mut temp_ti = [0i32; 500];
        let mut temp_tx = [0i32; 500];
        let mut temp_ty = [0i32; 500];
        let mut temp_tz = [0i32; 500];

        let mut s = store().lock().unwrap();
        if s.list.len() < total as usize + 1 {
            s.list.resize(total as usize + 1, None);
        }

        for _ in 0..total {
            let id = head.g2();
            let mut frame = AnimFrame {
                delay: del.g1(),
                base: Some(base.clone()),
                size: 0,
                ti: None,
                tx: None,
                ty: None,
                tz: None,
            };

            let group_count = head.g1();
            let mut last_group: i32 = -1;
            let mut current: usize = 0;

            let base_type = base.r#type.as_deref().unwrap_or(&[]);
            for j in 0..group_count as usize {
                let flags = tran1.g1();
                if flags > 0 {
                    if base_type.get(j).copied().unwrap_or(0) as i32 != 0 {
                        let mut group = j as i32 - 1;
                        while group > last_group {
                            if base_type.get(group as usize).copied().unwrap_or(0) as i32 == 0 {
                                temp_ti[current] = group;
                                temp_tx[current] = 0;
                                temp_ty[current] = 0;
                                temp_tz[current] = 0;
                                current += 1;
                                break;
                            }
                            group -= 1;
                        }
                    }

                    temp_ti[current] = j as i32;

                    let mut default_value = 0;
                    if base_type.get(temp_ti[current] as usize).copied().unwrap_or(0) as i32
                        == crate::dash3d::AnimTransform::SCALE
                    {
                        default_value = 128;
                    }

                    if flags & 0x1 == 0 {
                        temp_tx[current] = default_value;
                    } else {
                        temp_tx[current] = tran2.gsmarts();
                    }
                    if flags & 0x2 == 0 {
                        temp_ty[current] = default_value;
                    } else {
                        temp_ty[current] = tran2.gsmarts();
                    }
                    if flags & 0x4 == 0 {
                        temp_tz[current] = default_value;
                    } else {
                        temp_tz[current] = tran2.gsmarts();
                    }

                    last_group = j as i32;
                    current += 1;

                    if base_type.get(j).copied().unwrap_or(0) as i32
                        == crate::dash3d::AnimTransform::TRANSPARENCY
                    {
                        if let Some(opaque) = s.opaque.get_mut(id as usize) {
                            *opaque = false;
                        }
                    }
                }
            }

            frame.size = current as i32;
            frame.ti = Some(temp_ti[..current].to_vec());
            frame.tx = Some(temp_tx[..current].to_vec());
            frame.ty = Some(temp_ty[..current].to_vec());
            frame.tz = Some(temp_tz[..current].to_vec());

            s.list[id as usize] = Some(frame);
        }
    }

    /// `AnimFrame.get(id)` from client-ts; returns an owned copy of the
    /// frame (the store is process-wide, so callers cannot hold a borrow).
    pub fn get(id: i32) -> Option<AnimFrame> {
        let s = store().lock().unwrap();
        s.list.get(id as usize).and_then(|f| f.clone())
    }

    /// `AnimFrame.animateTransparencies(frame)` from client-ts.
    pub fn animate_transparencies(frame: i32) -> bool {
        frame == -1
    }
}
