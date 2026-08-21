// Port of `~/experiments/Server/webclient/src/config/SeqType.ts`.
use crate::io::{JagFile, Packet};

pub const PREANIM_DELAYMOVE: i32 = 0;
pub const PREANIM_DELAYANIM: i32 = 1;
pub const PREANIM_MERGE: i32 = 2;

pub const POSTANIM_DELAYMOVE: i32 = 0;
pub const POSTANIM_ABORTANIM: i32 = 1;
pub const POSTANIM_MERGE: i32 = 2;

pub const RESTART_RESET: i32 = 1;
pub const RESTART_RESETLOOP: i32 = 2;

#[derive(Clone)]
pub struct SeqType {
    pub num_frames: i32,
    pub frames: Option<Vec<i32>>,
    pub iframes: Option<Vec<i32>>,
    pub delay: Option<Vec<i32>>,
    pub loops: i32,
    pub walkmerge: Option<Vec<i32>>,
    pub reachforward: bool,
    pub priority: i32,
    pub replaceheldleft: i32,
    pub replaceheldright: i32,
    pub maxloops: i32,
    pub preanim_move: i32,
    pub postanim_move: i32,
    pub duplicatebehaviour: i32,
}

impl Default for SeqType {
    fn default() -> Self {
        SeqType {
            num_frames: 0,
            frames: None,
            iframes: None,
            delay: None,
            loops: -1,
            walkmerge: None,
            reachforward: false,
            priority: 5,
            replaceheldleft: -1,
            replaceheldright: -1,
            maxloops: 99,
            preanim_move: -1,
            postanim_move: -1,
            duplicatebehaviour: -1,
        }
    }
}

impl SeqType {
    pub fn unpack(jag: &JagFile) -> Vec<SeqType> {
        let Some(data) = jag.read("seq.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut list = Vec::with_capacity(num as usize);
        for _ in 0..num {
            let mut seq = SeqType::default();
            seq.decode(&mut dat);
            list.push(seq);
        }
        list
    }

    fn decode(&mut self, dat: &mut Packet) {
        loop {
            let code = dat.g1();
            if code == 0 {
                break;
            }
            match code {
                1 => {
                    // Int16Array assignment wraps g2 values to signed
                    self.num_frames = dat.g1();
                    let mut frames = Vec::with_capacity(self.num_frames as usize);
                    let mut iframes = Vec::with_capacity(self.num_frames as usize);
                    let mut delay = Vec::with_capacity(self.num_frames as usize);
                    for _ in 0..self.num_frames {
                        frames.push(dat.g2() as i16 as i32);
                        iframes.push(dat.g2() as i16 as i32);
                        delay.push(dat.g2() as i16 as i32);
                    }
                    self.frames = Some(frames);
                    self.iframes = Some(iframes);
                    self.delay = Some(delay);
                }
                2 => self.loops = dat.g2(),
                3 => {
                    let count = dat.g1();
                    let mut walkmerge = Vec::with_capacity(count as usize + 1);
                    for _ in 0..count {
                        walkmerge.push(dat.g1());
                    }
                    walkmerge.push(9999999);
                    self.walkmerge = Some(walkmerge);
                }
                4 => self.reachforward = true,
                5 => self.priority = dat.g1(),
                6 => self.replaceheldleft = dat.g2(),
                7 => self.replaceheldright = dat.g2(),
                8 => self.maxloops = dat.g1(),
                9 => self.preanim_move = dat.g1(),
                10 => self.postanim_move = dat.g1(),
                11 => self.duplicatebehaviour = dat.g1(),
                _ => eprintln!("Error unrecognised seq config code: {code}"),
            }
        }

        if self.num_frames == 0 {
            self.num_frames = 1;
            self.frames = Some(vec![-1]);
            self.iframes = Some(vec![-1]);
            self.delay = Some(vec![-1]);
        }

        if self.preanim_move == -1 {
            self.preanim_move = if self.walkmerge.is_none() {
                PREANIM_DELAYMOVE
            } else {
                PREANIM_MERGE
            };
        }

        if self.postanim_move == -1 {
            self.postanim_move = if self.walkmerge.is_none() {
                POSTANIM_DELAYMOVE
            } else {
                POSTANIM_MERGE
            };
        }
    }

    /// `getDelay(frame)` from client-ts. The TS memoises the resolved frame
    /// delay into `this.delay[frame]`; this port recomputes (same result),
    /// so it takes `&self`.
    pub fn get_delay(&self, frame: i32) -> i32 {
        let Some(delay) = self.delay.as_ref() else { return 0 };
        let Some(frames) = self.frames.as_ref() else { return 0 };

        let delay_value = if frame >= 0 && (frame as usize) < delay.len() {
            delay[frame as usize]
        } else {
            1
        };

        if delay_value == 0 {
            if let Some(&transform_id) = frames.get(frame.max(0) as usize) {
                if let Some(transform) = crate::dash3d::AnimFrame::get(transform_id) {
                    return transform.delay;
                }
            }
            return 1;
        }

        delay_value
    }
}
