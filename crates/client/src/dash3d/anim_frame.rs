// Port of `~/experiments/Server/webclient/src/dash3d/AnimFrame.ts`. The TS
// statics `list`/`opaque` live in a process-wide store; frames from one
// unpack share one private `AnimBase` allocation (matching TS object share).
// `get` returns a fully owned public clone so the store lock and private
// Arc never leak into callers.
use std::sync::{Arc, Mutex, OnceLock};

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

/// Public shape retained for API compatibility. The process-wide table uses
/// [`PrivateStore`] so multi-frame archives can share one `Arc<AnimBase>`.
pub struct AnimFrameStore {
    pub list: Vec<Option<AnimFrame>>,
    pub opaque: Vec<bool>,
}

/// Private published frame: frame-specific transforms plus a shared base Arc
/// from the unpack that created it. Incomplete public `AnimFrame` values
/// (base = None) are never stored or returned.
#[derive(Clone)]
struct PrivateFrame {
    delay: i32,
    base: Option<Arc<AnimBase>>,
    size: i32,
    ti: Option<Vec<i32>>,
    tx: Option<Vec<i32>>,
    ty: Option<Vec<i32>>,
    tz: Option<Vec<i32>>,
}

struct PrivateStore {
    list: Vec<Option<PrivateFrame>>,
    opaque: Vec<bool>,
}

// Process-wide by design: the decoded frame tables are the TS `AnimFrame`
// statics shared by every client; the data is immutable once `unpack`ed, so
// the `Mutex` only serialises concurrent access (see `Model::STORE`).
static STORE: OnceLock<Mutex<PrivateStore>> = OnceLock::new();

fn store() -> &'static Mutex<PrivateStore> {
    STORE.get_or_init(|| {
        Mutex::new(PrivateStore {
            list: Vec::new(),
            opaque: Vec::new(),
        })
    })
}

impl PrivateFrame {
    /// Materialize the public owned frame. Deep-clones the shared base and
    /// transform vectors; the returned value does not retain the Arc.
    fn to_public(&self) -> AnimFrame {
        AnimFrame {
            delay: self.delay,
            base: self.base.as_ref().map(|b| (**b).clone()),
            size: self.size,
            ti: self.ti.clone(),
            tx: self.tx.clone(),
            ty: self.ty.clone(),
            tz: self.tz.clone(),
        }
    }
}

impl AnimFrame {
    /// `AnimFrame.init(total)` from client-ts.
    ///
    /// Grow-only: a later client's `maininit` must not drop already-unpacked
    /// frames (the store is process-wide).
    pub fn init(total: i32) {
        let mut s = store().lock().unwrap();
        let need = total as usize + 1;
        if s.list.len() < need {
            s.list.resize(need, None);
        }
        if s.opaque.len() < need {
            s.opaque.resize(need, true);
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
        // One Arc per unpack invocation; frames from this archive clone the
        // Arc only. A later unpack with matching contents still allocates a
        // separate base (no cross-archive interning).
        let base = Arc::new(AnimBase::new(&mut base_buf));

        let total = head.g2();
        let mut temp_ti = [0i32; 500];
        let mut temp_tx = [0i32; 500];
        let mut temp_ty = [0i32; 500];
        let mut temp_tz = [0i32; 500];

        let mut s = store().lock().unwrap();
        // `total` is the number of frames in this archive entry, not the
        // global frame-id space. Cube seq 1133's first frame is 8483.
        // Grow to fit each `id` (and keep `opaque` in lockstep).
        if s.list.len() < total as usize + 1 {
            s.list.resize(total as usize + 1, None);
        }

        for _ in 0..total {
            let id = head.g2();
            let need = id as usize + 1;
            if s.list.len() < need {
                s.list.resize(need, None);
            }
            if s.opaque.len() < need {
                s.opaque.resize(need, true);
            }
            let mut frame = PrivateFrame {
                delay: del.g1(),
                base: Some(Arc::clone(&base)),
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
                    if base_type
                        .get(temp_ti[current] as usize)
                        .copied()
                        .unwrap_or(0) as i32
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
    /// The private Arc is not retained on the result.
    pub fn get(id: i32) -> Option<AnimFrame> {
        let s = store().lock().unwrap();
        s.list
            .get(id as usize)
            .and_then(Option::as_ref)
            .map(PrivateFrame::to_public)
    }

    /// Read only the delay under the store lock, without cloning transforms.
    /// Look up each time so later archive publication remains visible.
    pub(crate) fn delay(id: i32) -> Option<i32> {
        let s = store().lock().unwrap();
        s.list
            .get(id as usize)
            .and_then(Option::as_ref)
            .map(|frame| frame.delay)
    }

    /// `AnimFrame.animateTransparencies(frame)` from client-ts.
    pub fn animate_transparencies(frame: i32) -> bool {
        frame == -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash3d::model::Model;
    use crate::dash3d::AnimTransform;
    use std::sync::{Mutex, OnceLock};

    /// Serialize store-mutating fixtures: the animation store is process-wide.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn put_gsmarts(out: &mut Vec<u8>, v: i32) {
        if (-0x40..0x40).contains(&v) {
            out.push((v + 0x40) as u8);
        } else {
            let n = (v + 0xc000) as u16;
            out.extend_from_slice(&n.to_be_bytes());
        }
    }

    type FrameGroups<'a> = (u16, u8, &'a [(u8, i32, i32, i32)]);

    /// Build one OnDemand anim archive entry. Each frame is
    /// `(id, delay, &[(flags, tx, ty, tz); group_count])`.
    fn pack_archive(
        frames: &[FrameGroups<'_>],
        base_types: &[u8],
        base_labels: &[&[u8]],
    ) -> Vec<u8> {
        assert_eq!(base_types.len(), base_labels.len());
        let mut head = Vec::new();
        head.extend_from_slice(&(frames.len() as u16).to_be_bytes());
        let mut tran1 = Vec::new();
        let mut tran2 = Vec::new();
        let mut del = Vec::new();
        for &(id, delay, groups) in frames {
            head.extend_from_slice(&id.to_be_bytes());
            head.push(groups.len() as u8);
            del.push(delay);
            for &(flags, tx, ty, tz) in groups {
                tran1.push(flags);
                if flags > 0 {
                    if flags & 0x1 != 0 {
                        put_gsmarts(&mut tran2, tx);
                    }
                    if flags & 0x2 != 0 {
                        put_gsmarts(&mut tran2, ty);
                    }
                    if flags & 0x4 != 0 {
                        put_gsmarts(&mut tran2, tz);
                    }
                }
            }
        }
        let head_length = (head.len() - 2) as u16;
        let mut base = Vec::new();
        base.push(base_types.len() as u8);
        base.extend_from_slice(base_types);
        for labels in base_labels {
            base.push(labels.len() as u8);
            base.extend_from_slice(labels);
        }
        let mut out = Vec::new();
        out.extend_from_slice(&head);
        out.extend_from_slice(&tran1);
        out.extend_from_slice(&tran2);
        out.extend_from_slice(&del);
        out.extend_from_slice(&base);
        out.extend_from_slice(&head_length.to_be_bytes());
        out.extend_from_slice(&(tran1.len() as u16).to_be_bytes());
        out.extend_from_slice(&(tran2.len() as u16).to_be_bytes());
        out.extend_from_slice(&(del.len() as u16).to_be_bytes());
        out
    }

    /// Isolated high IDs for this suite (avoid 0 / 30001 used elsewhere).
    const ID_A: u16 = 41010;
    const ID_B: u16 = 41011;
    const ID_C: u16 = 41012;
    const ID_HIGH: u16 = 42000;
    const ID_REP_A: u16 = 41020;
    const ID_REP_B: u16 = 41021;

    #[test]
    fn public_multi_frame_defaults_origin_signed_and_transparency() {
        let _g = test_lock();
        // groups: ORIGIN, TRANSLATE, SCALE, TRANSPARENCY
        let types = [
            AnimTransform::ORIGIN as u8,
            AnimTransform::TRANSLATE as u8,
            AnimTransform::SCALE as u8,
            AnimTransform::TRANSPARENCY as u8,
        ];
        let labels: &[&[u8]] = &[&[0], &[1], &[0], &[2]];
        // Frame A: skip origin stream entry; translate signed; scale defaults via flags=8;
        // transparency x-only.
        let groups_a: &[(u8, i32, i32, i32)] = &[
            (0, 0, 0, 0),
            (0x7, 1, -2, 3),
            (0x8, 0, 0, 0),
            (0x1, 0, 0, 0),
        ];
        // Frame B: different delay/transforms, same base contents.
        let groups_b: &[(u8, i32, i32, i32)] =
            &[(0, 0, 0, 0), (0x1, -5, 0, 0), (0x8, 0, 0, 0), (0, 0, 0, 0)];
        AnimFrame::unpack(&pack_archive(
            &[(ID_A, 3, groups_a), (ID_B, 9, groups_b)],
            &types,
            labels,
        ));

        let a = AnimFrame::get(ID_A as i32).expect("frame A");
        let b = AnimFrame::get(ID_B as i32).expect("frame B");
        assert_eq!(a.delay, 3);
        assert_eq!(b.delay, 9);
        // Origin auto-inserted before translate when type[j] != ORIGIN.
        assert_eq!(a.ti.as_deref(), Some([0, 1, 2, 3].as_slice()));
        assert_eq!(a.tx.as_deref(), Some([0, 1, 128, 0].as_slice()));
        // Transparency is not SCALE, so missing axes default to 0 (not 128).
        assert_eq!(a.ty.as_deref(), Some([0, -2, 128, 0].as_slice()));
        assert_eq!(a.tz.as_deref(), Some([0, 3, 128, 0].as_slice()));
        assert_eq!(a.size, 4);
        assert_eq!(b.ti.as_deref(), Some([0, 1, 2].as_slice()));
        assert_eq!(b.tx.as_deref(), Some([0, -5, 128].as_slice()));
        assert_eq!(b.ty.as_deref(), Some([0, 0, 128].as_slice()));
        assert_eq!(b.tz.as_deref(), Some([0, 0, 128].as_slice()));

        let base_a = a.base.as_ref().expect("base A");
        let base_b = b.base.as_ref().expect("base B");
        assert_eq!(base_a.size, 4);
        assert_eq!(base_a.r#type.as_deref(), Some(types.as_slice()));
        assert_eq!(
            base_a.labels.as_ref().map(|l| {
                l.iter()
                    .map(|g| g.as_deref().unwrap_or(&[]).to_vec())
                    .collect::<Vec<_>>()
            }),
            Some(vec![vec![0], vec![1], vec![0], vec![2]])
        );
        // Public contents match across frames from one archive.
        assert_eq!(base_a.r#type, base_b.r#type);
        assert_eq!(base_a.labels, base_b.labels);
        assert_eq!(base_a.size, base_b.size);

        // Transparency group marks the frame non-opaque.
        {
            let s = store().lock().unwrap();
            assert_eq!(s.opaque.get(ID_A as usize).copied(), Some(false));
            // Frame B never hit the transparency group.
            assert_eq!(s.opaque.get(ID_B as usize).copied(), Some(true));
        }

        // Model vertex translate via frame A group 1 label 1.
        let mut model = Model {
            num_points: 2,
            point_x: Some(vec![10, 100]),
            point_y: Some(vec![20, 200]),
            point_z: Some(vec![30, 300]),
            label_vertices: Some(vec![
                Some(vec![0]), // label 0
                Some(vec![1]), // label 1 → point 1
                Some(vec![0]),
            ]),
            ..Default::default()
        };
        model.animate(ID_A as i32);
        // ORIGIN (label 0) updates origin only; TRANSLATE applies (1,-2,3) to label 1.
        // SCALE on label 0 with defaults 128 is identity relative to origin.
        assert_eq!(model.point_x.as_deref(), Some([10, 101].as_slice()));
        assert_eq!(model.point_y.as_deref(), Some([20, 198].as_slice()));
        assert_eq!(model.point_z.as_deref(), Some([30, 303].as_slice()));
    }

    #[test]
    fn public_get_results_are_independent_nested_and_transforms() {
        let _g = test_lock();
        let types = [AnimTransform::TRANSLATE as u8];
        let labels: &[&[u8]] = &[&[0, 1]];
        let groups: &[(u8, i32, i32, i32)] = &[(0x7, 4, 5, 6)];
        AnimFrame::unpack(&pack_archive(
            &[(ID_C, 1, groups), (ID_A, 2, groups)],
            &types,
            labels,
        ));

        let mut first = AnimFrame::get(ID_C as i32).unwrap();
        let sibling_before = AnimFrame::get(ID_A as i32).unwrap();
        first.tx.as_mut().unwrap()[0] = 99;
        first.ty.as_mut().unwrap()[0] = 98;
        first.tz.as_mut().unwrap()[0] = 97;
        first.ti.as_mut().unwrap()[0] = 77;
        let base = first.base.as_mut().unwrap();
        base.r#type.as_mut().unwrap()[0] = 255;
        base.labels.as_mut().unwrap()[0].as_mut().unwrap().push(9);
        base.size = 99;

        let again = AnimFrame::get(ID_C as i32).unwrap();
        assert_eq!(again.tx.as_deref(), Some([4].as_slice()));
        assert_eq!(again.ty.as_deref(), Some([5].as_slice()));
        assert_eq!(again.tz.as_deref(), Some([6].as_slice()));
        assert_eq!(again.ti.as_deref(), Some([0].as_slice()));
        let base2 = again.base.as_ref().unwrap();
        assert_eq!(base2.r#type.as_deref(), Some([1u8].as_slice()));
        assert_eq!(
            base2.labels.as_ref().unwrap()[0].as_deref(),
            Some([0u8, 1u8].as_slice())
        );
        assert_eq!(base2.size, 1);

        let sibling = AnimFrame::get(ID_A as i32).unwrap();
        assert_eq!(sibling.tx, sibling_before.tx);
        assert_eq!(
            sibling.base.as_ref().unwrap().r#type,
            sibling_before.base.as_ref().unwrap().r#type
        );
        assert_eq!(
            sibling.base.as_ref().unwrap().labels,
            sibling_before.base.as_ref().unwrap().labels
        );
    }

    #[test]
    fn public_partial_republish_preserves_old_result_and_sibling() {
        let _g = test_lock();
        let types_old = [AnimTransform::TRANSLATE as u8];
        let labels_old: &[&[u8]] = &[&[3]];
        let g: &[(u8, i32, i32, i32)] = &[(0x1, 11, 0, 0)];
        AnimFrame::unpack(&pack_archive(
            &[(ID_REP_A, 4, g), (ID_REP_B, 5, g)],
            &types_old,
            labels_old,
        ));
        let old_a = AnimFrame::get(ID_REP_A as i32).unwrap();
        let old_b = AnimFrame::get(ID_REP_B as i32).unwrap();
        assert_eq!(old_a.delay, 4);
        assert_eq!(old_a.tx.as_deref(), Some([11].as_slice()));
        assert_eq!(
            old_a.base.as_ref().unwrap().labels.as_ref().unwrap()[0].as_deref(),
            Some([3u8].as_slice())
        );

        // Replace only ID_REP_A with a different base and delay.
        let types_new = [AnimTransform::SCALE as u8, AnimTransform::TRANSLATE as u8];
        let labels_new: &[&[u8]] = &[&[0], &[1]];
        let g_new: &[(u8, i32, i32, i32)] = &[(0x8, 0, 0, 0), (0x2, 0, 7, 0)];
        AnimFrame::unpack(&pack_archive(
            &[(ID_REP_A, 8, g_new)],
            &types_new,
            labels_new,
        ));

        // Previously returned owned frame is detached.
        assert_eq!(old_a.delay, 4);
        assert_eq!(old_a.tx.as_deref(), Some([11].as_slice()));
        assert_eq!(
            old_a.base.as_ref().unwrap().r#type.as_deref(),
            Some([1u8].as_slice())
        );

        let new_a = AnimFrame::get(ID_REP_A as i32).unwrap();
        assert_eq!(new_a.delay, 8);
        assert_eq!(new_a.base.as_ref().unwrap().size, 2);
        assert_eq!(
            new_a.base.as_ref().unwrap().r#type.as_deref(),
            Some([3u8, 1u8].as_slice())
        );

        // Untouched sibling keeps prior archive contents.
        let still_b = AnimFrame::get(ID_REP_B as i32).unwrap();
        assert_eq!(still_b.delay, old_b.delay);
        assert_eq!(still_b.tx, old_b.tx);
        assert_eq!(
            still_b.base.as_ref().unwrap().labels,
            old_b.base.as_ref().unwrap().labels
        );
        assert_eq!(
            still_b.base.as_ref().unwrap().r#type,
            old_b.base.as_ref().unwrap().r#type
        );
    }

    #[test]
    fn public_init_high_id_missing_and_struct_construction() {
        let _g = test_lock();
        let types = [AnimTransform::TRANSLATE as u8];
        let labels: &[&[u8]] = &[&[0]];
        let g: &[(u8, i32, i32, i32)] = &[(0x1, 1, 0, 0)];
        AnimFrame::unpack(&pack_archive(&[(ID_HIGH, 6, g)], &types, labels));
        assert!(AnimFrame::get(ID_HIGH as i32).is_some());
        assert_eq!(AnimFrame::delay(ID_HIGH as i32), Some(6));

        // Grow-only init must not drop the high-ID frame.
        AnimFrame::init(16);
        assert!(AnimFrame::get(ID_HIGH as i32).is_some());
        assert_eq!(AnimFrame::delay(ID_HIGH as i32), Some(6));

        assert!(AnimFrame::get(-1).is_none());
        assert!(AnimFrame::get(i32::MAX).is_none());
        assert_eq!(AnimFrame::delay(-1), None);
        assert_eq!(AnimFrame::delay(ID_HIGH as i32 + 50), None);
        // Missing slot inside a grown table.
        assert!(AnimFrame::get(ID_HIGH as i32 - 1).is_none());

        // Public structs remain constructible with owned fields.
        let base = AnimBase {
            size: 1,
            r#type: Some(vec![1]),
            labels: Some(vec![Some(vec![0])]),
        };
        let frame = AnimFrame {
            delay: 1,
            base: Some(base.clone()),
            size: 0,
            ti: Some(vec![]),
            tx: Some(vec![]),
            ty: Some(vec![]),
            tz: Some(vec![]),
        };
        assert_eq!(frame.base.as_ref().unwrap().size, 1);
        let _store_shape = AnimFrameStore {
            list: vec![Some(frame)],
            opaque: vec![true],
        };
        assert!(AnimFrame::animate_transparencies(-1));
        assert!(!AnimFrame::animate_transparencies(0));
    }

    /// IDs reserved for private Arc lifetime proofs (no overlap with public fixtures).
    const OWN_A: u16 = 43010;
    const OWN_B: u16 = 43011;
    const OWN_C: u16 = 43012;
    const OWN_D: u16 = 43013;
    const OWN_E: u16 = 43014;

    fn private_base(id: i32) -> Option<Arc<AnimBase>> {
        let s = store().lock().unwrap();
        s.list
            .get(id as usize)
            .and_then(Option::as_ref)
            .and_then(|f| f.base.clone())
    }

    #[test]
    fn private_arc_shared_within_unpack_not_across_and_get_detaches() {
        let _g = test_lock();
        let types = [AnimTransform::TRANSLATE as u8];
        let labels: &[&[u8]] = &[&[0]];
        let g: &[(u8, i32, i32, i32)] = &[(0x1, 2, 0, 0)];
        AnimFrame::unpack(&pack_archive(
            &[(OWN_A, 1, g), (OWN_B, 2, g), (OWN_C, 3, g)],
            &types,
            labels,
        ));

        let a = private_base(OWN_A as i32).expect("OWN_A base");
        let b = private_base(OWN_B as i32).expect("OWN_B base");
        let c = private_base(OWN_C as i32).expect("OWN_C base");
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&b, &c));
        // Three private records hold the Arc; local test clones bump the count.
        assert_eq!(Arc::strong_count(&a), 6); // 3 store + 3 locals
        drop((b, c));
        assert_eq!(Arc::strong_count(&a), 4); // 3 store + this local

        // Public get deep-materializes and does not retain the Arc.
        let before = Arc::strong_count(&a);
        let owned = AnimFrame::get(OWN_A as i32).unwrap();
        assert!(owned.base.is_some());
        assert_eq!(Arc::strong_count(&a), before);
        drop(owned);
        assert_eq!(Arc::strong_count(&a), before);

        // Separate unpack with identical contents → distinct Arc.
        AnimFrame::unpack(&pack_archive(&[(OWN_D, 4, g)], &types, labels));
        let d = private_base(OWN_D as i32).unwrap();
        assert!(!Arc::ptr_eq(&a, &d));
        assert_eq!(d.r#type, a.r#type);
    }

    #[test]
    fn private_partial_replace_keeps_old_arc_until_final_owner_drops() {
        let _g = test_lock();
        let types = [AnimTransform::TRANSLATE as u8];
        let labels: &[&[u8]] = &[&[9]];
        let g: &[(u8, i32, i32, i32)] = &[(0x1, 3, 0, 0)];
        AnimFrame::unpack(&pack_archive(
            &[(OWN_A, 1, g), (OWN_B, 1, g), (OWN_E, 1, g)],
            &types,
            labels,
        ));
        let shared = private_base(OWN_A as i32).unwrap();
        let weak = Arc::downgrade(&shared);
        drop(shared);
        assert!(weak.upgrade().is_some());

        // Replace one frame with a different archive base.
        let types2 = [AnimTransform::SCALE as u8];
        let labels2: &[&[u8]] = &[&[1]];
        let g2: &[(u8, i32, i32, i32)] = &[(0x8, 0, 0, 0)];
        AnimFrame::unpack(&pack_archive(&[(OWN_A, 9, g2)], &types2, labels2));
        let still = weak.upgrade().expect("sibling owners keep old base alive");
        // OWN_B + OWN_E in store, plus this local upgrade handle.
        assert_eq!(Arc::strong_count(&still), 3);
        assert!(private_base(OWN_B as i32).is_some_and(|b| Arc::ptr_eq(&b, &still)));
        assert!(!private_base(OWN_A as i32).is_some_and(|b| Arc::ptr_eq(&b, &still)));

        // Replace remaining owners → final drop.
        AnimFrame::unpack(&pack_archive(
            &[(OWN_B, 9, g2), (OWN_E, 9, g2)],
            &types2,
            labels2,
        ));
        // Store no longer holds the old Arc; only `still` remains.
        assert_eq!(Arc::strong_count(&still), 1);
        drop(still);
        assert!(
            weak.upgrade().is_none(),
            "old base must drop when last private record is replaced"
        );
    }
}
