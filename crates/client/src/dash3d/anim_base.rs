// Port of `~/experiments/Server/webclient/src/dash3d/AnimBase.ts`.
use crate::io::Packet;

pub struct AnimTransform;

impl AnimTransform {
    pub const ORIGIN: i32 = 0;
    pub const TRANSLATE: i32 = 1;
    pub const ROTATE: i32 = 2;
    pub const SCALE: i32 = 3;
    pub const TRANSPARENCY: i32 = 5;
}

#[derive(Clone, Default)]
pub struct AnimBase {
    pub size: i32,
    pub r#type: Option<Vec<u8>>,
    pub labels: Option<Vec<Option<Vec<u8>>>>,
}

impl AnimBase {
    pub fn new(buf: &mut Packet) -> Self {
        let size = buf.g1();
        let mut r#type = vec![0u8; size as usize];
        for t in &mut r#type {
            *t = buf.g1() as u8;
        }
        let mut labels = Vec::with_capacity(size as usize);
        for _ in 0..size {
            let count = buf.g1();
            let mut group = vec![0u8; count as usize];
            for g in &mut group {
                *g = buf.g1() as u8;
            }
            labels.push(Some(group));
        }
        AnimBase { size, r#type: Some(r#type), labels: Some(labels) }
    }
}
