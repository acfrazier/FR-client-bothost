//! Client-Java 377 `config/SeqType.java`: defaults 10-56, decoder 92-165.
//! Java stores unsigned g2 frame words in int[], not 289's signed TS arrays.

use super::{DecodeError377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct SeqType377 {
    pub num_frames: i32,
    pub frames: Vec<i32>,
    pub iframes: Vec<i32>,
    pub delay: Vec<i32>,
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
    /// Opcode 12, Java field790 (not used by the pinned client).
    pub reserved: i32,
}

impl Default for SeqType377 {
    fn default() -> Self {
        Self {
            num_frames: 0,
            frames: Vec::new(),
            iframes: Vec::new(),
            delay: Vec::new(),
            loops: -1,
            walkmerge: None,
            reachforward: false,
            priority: 5,
            replaceheldleft: -1,
            replaceheldright: -1,
            maxloops: 99,
            preanim_move: -1,
            postanim_move: -1,
            duplicatebehaviour: 2,
            reserved: 0,
        }
    }
}

impl SeqType377 {
    pub fn decode(dat: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut seq = Self::default();
        loop {
            let code = dat.g1()?;
            match code {
                0 => break,
                1 => {
                    seq.num_frames = dat.g1()?;
                    seq.frames = Vec::with_capacity(seq.num_frames as usize);
                    seq.iframes = Vec::with_capacity(seq.num_frames as usize);
                    seq.delay = Vec::with_capacity(seq.num_frames as usize);
                    for _ in 0..seq.num_frames {
                        seq.frames.push(dat.g2()?);
                        let iframe = dat.g2()?;
                        seq.iframes.push(if iframe == 65535 { -1 } else { iframe });
                        seq.delay.push(dat.g2()?);
                    }
                }
                2 => seq.loops = dat.g2()?,
                3 => {
                    let count = dat.g1()?;
                    let mut walkmerge = Vec::with_capacity(count as usize + 1);
                    for _ in 0..count {
                        walkmerge.push(dat.g1()?);
                    }
                    walkmerge.push(9_999_999);
                    seq.walkmerge = Some(walkmerge);
                }
                4 => seq.reachforward = true,
                5 => seq.priority = dat.g1()?,
                6 => seq.replaceheldleft = dat.g2()?,
                7 => seq.replaceheldright = dat.g2()?,
                8 => seq.maxloops = dat.g1()?,
                9 => seq.preanim_move = dat.g1()?,
                10 => seq.postanim_move = dat.g1()?,
                11 => seq.duplicatebehaviour = dat.g1()?,
                12 => seq.reserved = dat.g4()?,
                _ => return Err(dat.unknown(code)),
            }
        }
        // SeqType.java:95-120, including the sentinel frame and merge defaults.
        if seq.num_frames == 0 {
            seq.num_frames = 1;
            seq.frames = vec![-1];
            seq.iframes = vec![-1];
            seq.delay = vec![-1];
        }
        if seq.preanim_move == -1 {
            seq.preanim_move = if seq.walkmerge.is_some() { 2 } else { 0 };
        }
        if seq.postanim_move == -1 {
            seq.postanim_move = if seq.walkmerge.is_some() { 2 } else { 0 };
        }
        Ok(seq)
    }
}
