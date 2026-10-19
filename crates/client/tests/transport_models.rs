//! Bounded explicit-state models for the proposed client transport cutover.
//!
//! Assumptions: one worker, one reactor, one producer, two transfer identities,
//! and at most two read and two write readiness publications per identity.
//! The fake monotonic clock uses 0, 5, 25, 29, and 30 seconds. Login deadline D
//! is 30 seconds and the lane-wait start floor is 5 seconds. Counters never wrap.
//! The scheduler executes one enabled owner operation at a time. It does not use
//! threads or sleep. A native call may return WouldBlock, a synthetic limit,
//! success, or error, or remain Running forever. Liveness assumes fair scheduling
//! and a returning native call; a permanently Running call has no return-liveness
//! obligation. TCP does not use the TLS handshake lane. Queue/lease credit and
//! physical lane/socket credit are distinct resources.
//!
//! G1 maps to `transport::stream_handle::Park::wait`: its ready closure may
//! call `SharedConnection::publish_record` only under K2. `CancelToken::cancel`
//! publishes its fired flag under K2. The Mio kick occurs after K2 unlock.
//! Runtime operation names below also map to the gate's TLS handoff lock,
//! ready_seq and pre-call snapshots, runnable flag, worker epoch, atomic condvar
//! release, result cell, mio wake token, K2 capacity marker, initial-TLS receipt,
//! and per-attempt retirement cell. These finite models do not claim that runtime
//! implementations already satisfy the protocol.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

fn bump(value: u8) -> u8 {
    value.checked_add(1).expect("bounded model counter wrapped")
}

fn shortest_counterexample<S, A, Next, Bad, Label>(
    start: S,
    mut next: Next,
    mut bad: Bad,
    mut label: Label,
    state_limit: usize,
) -> (Vec<String>, usize)
where
    S: Copy + Eq + Hash,
    A: Copy,
    Next: FnMut(&S) -> Vec<(S, A)>,
    Bad: FnMut(&S) -> bool,
    Label: FnMut(A) -> String,
{
    let mut seen = HashMap::<S, Option<(S, A)>>::new();
    let mut queue = VecDeque::new();
    seen.insert(start, None);
    queue.push_back(start);

    while let Some(state) = queue.pop_front() {
        if bad(&state) {
            let mut path = Vec::new();
            let mut cursor = state;
            while cursor != start {
                let (previous, action) = seen[&cursor].expect("non-root state has a parent");
                path.push(label(action));
                cursor = previous;
            }
            path.reverse();
            return (path, seen.len());
        }
        for (successor, action) in next(&state) {
            if !seen.contains_key(&successor) {
                assert!(seen.len() < state_limit, "mutant state bound exceeded");
                seen.insert(successor, Some((state, action)));
                queue.push_back(successor);
            }
        }
    }
    (Vec::new(), seen.len())
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum G1Action {
    SlotLock,
    SlotArmUnderK2,
    SlotRecheckSpace,
    SlotCondvarRelease,
    SlotWakeAndReacquire,
    SlotRecheckAfterWake,
    ReactorPublishSpace,
    ReactorLockK2,
    ReactorReadMarkerUnderK2,
    ReactorUnlockK2,
    ReactorNotify,
    CancelTokenLockK2,
    CancelTokenPublishFiredUnderK2,
    CancelTokenNotifyAfterUnlock,
    SlotKickMioAfterK2Unlock,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct G1State {
    slot: u8,
    reactor: u8,
    k2: u8,
    marker: bool,
    space: bool,
    record_published: bool,
    sleeping: bool,
    notify_pending: bool,
    wake_pending: bool,
    cancel_fired: bool,
    cancel_notify_pending: bool,
    cancel_rechecked: bool,
    mio_kick_pending: bool,
    mio_kicked: bool,
    epoch: u8,
    drain_before_arm: bool,
    drain_after_recheck: bool,
}

impl G1State {
    fn initial() -> Self {
        Self {
            slot: 0,
            reactor: 0,
            k2: 0,
            marker: false,
            space: false,
            record_published: false,
            mio_kick_pending: false,
            mio_kicked: false,
            sleeping: false,
            notify_pending: false,
            wake_pending: false,
            epoch: 0,
            cancel_fired: false,
            cancel_notify_pending: false,
            cancel_rechecked: false,
            drain_before_arm: false,
            drain_after_recheck: false,
        }
    }

    fn successors(self) -> Vec<(Self, G1Action)> {
        let mut out = Vec::new();
        if self.slot == 0 && self.k2 == 0 {
            let mut next = self;
            next.k2 = 1;
            next.slot = 1;
            out.push((next, G1Action::SlotLock));
        }
        if self.slot == 1 && self.k2 == 1 {
            let mut next = self;
            next.marker = true;
            next.slot = 2;
            if next.space {
                next.drain_before_arm = true;
            }
            out.push((next, G1Action::SlotArmUnderK2));
        }
        if self.slot == 2 && self.k2 == 1 {
            let mut next = self;
            if self.cancel_fired {
                next.marker = false;
                next.cancel_rechecked = true;
                next.k2 = 0;
                next.slot = 6;
            } else if self.space {
                // This is SharedConnection::publish_record inside Park::wait's
                // K2-ready closure, before K2 unlock and the later Mio kick.
                next.record_published = true;
                next.mio_kick_pending = true;
                next.marker = false;
                next.k2 = 0;
                next.slot = 6;
            } else {
                next.slot = 3;
            }
            out.push((next, G1Action::SlotRecheckSpace));
        }
        if self.slot == 3 && self.k2 == 1 {
            let mut next = self;
            next.k2 = 0;
            next.sleeping = true;
            next.slot = 4;
            out.push((next, G1Action::SlotCondvarRelease));
        }
        if self.slot == 4 && self.wake_pending && self.k2 == 0 {
            let mut next = self;
            next.wake_pending = false;
            next.sleeping = false;
            next.k2 = 1;
            next.slot = 5;
            out.push((next, G1Action::SlotWakeAndReacquire));
        }
        if self.slot == 5 && self.k2 == 1 {
            let mut next = self;
            if self.cancel_fired {
                next.marker = false;
                next.cancel_rechecked = true;
                next.k2 = 0;
                next.slot = 6;
            } else if self.space {
                next.record_published = true;
                next.mio_kick_pending = true;
                next.marker = false;
                next.k2 = 0;
                next.slot = 6;
            } else {
                next.k2 = 0;
                next.sleeping = true;
                next.slot = 4;
            }
            out.push((next, G1Action::SlotRecheckAfterWake));
        }
        if self.k2 == 0 && !self.cancel_fired {
            let mut next = self;
            next.k2 = 3;
            out.push((next, G1Action::CancelTokenLockK2));
        }
        if self.k2 == 3 && !self.cancel_fired {
            let mut next = self;
            next.cancel_fired = true;
            next.k2 = 0;
            if self.sleeping {
                next.cancel_notify_pending = true;
            }
            out.push((next, G1Action::CancelTokenPublishFiredUnderK2));
        }
        if self.cancel_notify_pending {
            let mut next = self;
            next.cancel_notify_pending = false;
            if next.sleeping {
                next.sleeping = false;
                next.wake_pending = true;
            }
            out.push((next, G1Action::CancelTokenNotifyAfterUnlock));
        }
        if self.mio_kick_pending {
            let mut next = self;
            next.mio_kick_pending = false;
            next.mio_kicked = true;
            out.push((next, G1Action::SlotKickMioAfterK2Unlock));
        }
        if self.reactor == 0 {
            let mut next = self;
            next.space = true;
            next.reactor = 1;
            if self.slot == 3 || self.slot == 4 {
                next.drain_after_recheck = true;
            }
            out.push((next, G1Action::ReactorPublishSpace));
        }
        if self.reactor == 1 && self.k2 == 0 {
            let mut next = self;
            next.k2 = 2;
            next.reactor = 2;
            out.push((next, G1Action::ReactorLockK2));
        }
        if self.reactor == 2 && self.k2 == 2 {
            let mut next = self;
            if self.marker {
                next.epoch = bump(self.epoch);
                next.notify_pending = true;
            }
            next.reactor = 3;
            out.push((next, G1Action::ReactorReadMarkerUnderK2));
        }
        if self.reactor == 3 && self.k2 == 2 {
            let mut next = self;
            next.k2 = 0;
            next.reactor = 4;
            out.push((next, G1Action::ReactorUnlockK2));
        }
        if self.reactor == 4 && self.notify_pending {
            let mut next = self;
            next.notify_pending = false;
            if next.sleeping {
                next.sleeping = false;
                next.wake_pending = true;
            }
            out.push((next, G1Action::ReactorNotify));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LitmusAction {
    SlotStoreMarker,
    ReactorStoreSpace,
    SlotLoadStaleSpace,
    ReactorLoadStaleMarker,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct WeakAtomicLitmus {
    marker_stored: bool,
    space_stored: bool,
    slot_load_done: bool,
    reactor_load_done: bool,
    slot_saw_space: bool,
    reactor_saw_marker: bool,
}

impl WeakAtomicLitmus {
    fn successors(self) -> Vec<(Self, LitmusAction)> {
        let mut out = Vec::new();
        if !self.marker_stored {
            let mut next = self;
            next.marker_stored = true;
            out.push((next, LitmusAction::SlotStoreMarker));
        }
        if !self.space_stored {
            let mut next = self;
            next.space_stored = true;
            out.push((next, LitmusAction::ReactorStoreSpace));
        }
        if self.marker_stored && !self.slot_load_done {
            let mut next = self;
            // This stale read is allowed by the weak-atomic mutant because the
            // producer's store is not ordered by K2 with the consumer's load.
            next.slot_load_done = true;
            next.slot_saw_space = false;
            out.push((next, LitmusAction::SlotLoadStaleSpace));
        }
        if self.space_stored && !self.reactor_load_done {
            let mut next = self;
            next.reactor_load_done = true;
            next.reactor_saw_marker = false;
            out.push((next, LitmusAction::ReactorLoadStaleMarker));
        }
        out
    }
}

#[test]
fn g1_m_k2_mutex_condvar_model_and_weak_marker_mutant() {
    let start = G1State::initial();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut saw_drain_before_arm = false;
    let mut saw_drain_after_recheck = false;
    let mut saw_record_publication = false;
    let mut saw_cancel_recheck = false;
    let mut saw_mio_kick = false;
    seen.insert(start);
    queue.push_back(start);
    while let Some(state) = queue.pop_front() {
        saw_drain_before_arm |= state.drain_before_arm;
        saw_drain_after_recheck |= state.drain_after_recheck;
        saw_record_publication |= state.record_published;
        saw_cancel_recheck |= state.cancel_rechecked;
        saw_mio_kick |= state.mio_kicked;
        if state.sleeping && state.space && !state.wake_pending {
            assert!(
                state.notify_pending || state.cancel_notify_pending || state.reactor < 4,
                "usable space cannot strand a sleeping worker without a pending wake"
            );
        }
        assert!(
            !state.mio_kick_pending || state.k2 != 1,
            "the slot's Mio kick is pending only after its K2 unlock"
        );
        if state.slot == 6 && state.reactor == 4 && !state.mio_kick_pending {
            assert!(!state.sleeping || !state.space || state.wake_pending);
            assert!(
                !state.marker,
                "slot clears its marker on every terminal path"
            );
            assert_ne!(state.k2, 1, "terminal slot does not retain K2");
            assert!(!state.record_published || state.mio_kicked);
        }
        for (next, _) in state.successors() {
            if seen.insert(next) {
                assert!(seen.len() < 10_000, "G1-M finite state bound exceeded");
                queue.push_back(next);
            }
        }
    }
    assert!(saw_drain_before_arm, "covers drain before slot arm/recheck");
    assert!(saw_drain_after_recheck, "covers drain after slot recheck");
    assert!(
        saw_record_publication,
        "record publication runs inside the K2-ready closure"
    );
    assert!(
        saw_cancel_recheck,
        "CancelToken::cancel fires under K2 before the Park recheck"
    );
    assert!(
        saw_mio_kick,
        "record publication kicks Mio only after K2 unlock"
    );

    let litmus = WeakAtomicLitmus {
        marker_stored: false,
        space_stored: false,
        slot_load_done: false,
        reactor_load_done: false,
        slot_saw_space: false,
        reactor_saw_marker: false,
    };
    let (counterexample, mutant_states) = shortest_counterexample(
        litmus,
        |state| state.successors(),
        |state| {
            state.slot_load_done
                && state.reactor_load_done
                && !state.slot_saw_space
                && !state.reactor_saw_marker
        },
        |action| format!("{action:?}"),
        100,
    );
    let expected = vec![
        "SlotStoreMarker".to_owned(),
        "ReactorStoreSpace".to_owned(),
        "SlotLoadStaleSpace".to_owned(),
        "ReactorLoadStaleMarker".to_owned(),
    ];
    assert_eq!(
        counterexample, expected,
        "shortest R1 wrong-marker-read trace"
    );
    println!(
        "G1-M explored_states={} wrong_marker_mutant_states={} K2_record_cancel_and_post_unlock_kick=true shortest={}",
        seen.len(),
        mutant_states,
        counterexample.join(" -> ")
    );
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Owner {
    None,
    Worker,
    ResultCell,
    Reactor,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TxPhase {
    Empty,
    Ready,
    Calling,
    WouldBlock,
    LockedRecheck,
    NeedCondvarRelease,
    Waiting,
    Runnable,
    Returned,
    CellPublished,
    Taken,
    Accepted,
    Released,
    Stuck,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum TxAction {
    AdmitFirst,
    PublishReadEdge,
    PublishWriteEdge,
    NotifyRead,
    NotifyWrite,
    WorkerCall,
    NativeWouldBlock,
    NativeSyntheticLimit,
    NativeSuccess,
    NativeError,
    NativeRemainsRunning,
    LockForRecheck,
    LockedRecheck,
    AtomicCondvarRelease,
    CancellationWake,
    LockForResult,
    PublishResultCell,
    MioWake,
    PollSleep,
    PollWake,
    PollTimeout,
    PollDrain,
    TakeResult,
    AcceptExactTransfer,
    RejectAndClose,
    WorkerCloseDrop,
    AdvanceToFive,
    AdvanceToDeadline,
    RevokeCancel,
    RevokeDeadline,
    RevokeShutdown,
    AdmitSecondIdentity,
}

const C_WOULDBLOCK: u32 = 1 << 0;
const C_EDGE_BEFORE_WB: u32 = 1 << 1;
const C_EDGE_AFTER_WB: u32 = 1 << 2;
const C_WORKER_SLEPT: u32 = 1 << 3;
const C_WORKER_WOKE: u32 = 1 << 4;
const C_SYNTHETIC: u32 = 1 << 5;
const C_SUCCESS: u32 = 1 << 6;
const C_ERROR: u32 = 1 << 7;
const C_STUCK: u32 = 1 << 8;
const C_POLL_TIMEOUT: u32 = 1 << 9;
const C_WAKE_COALESCED: u32 = 1 << 10;
const C_TWO_ACCEPTS: u32 = 1 << 11;
const C_CANCEL: u32 = 1 << 12;
const C_DEADLINE: u32 = 1 << 13;
const C_SHUTDOWN: u32 = 1 << 14;
const MAX_WORKER_EPOCH: u8 = 10; // two identities × (four readiness edges + one revocation)

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TxState {
    current: u8,
    owners: [Owner; 2],
    phase: TxPhase,
    transfer_seq: u8,
    binding_generation: u8,
    ready_seq: [u8; 2],
    pre_call_snapshot: [u8; 2],
    runnable: bool,
    worker_waiting: bool,
    worker_epoch: u8,
    tls_lock: bool,
    cancel_notify_pending: bool,
    notify_pending: [bool; 2],
    calls: u8,
    would_block_seen: bool,
    result_kind: u8,
    result_cell: u8,
    result_identity: u8,
    result_binding: u8,
    result_transfer_seq: u8,
    completion_hint: bool,
    poll_asleep: bool,
    wake_token: bool,
    timeout_armed: bool,
    lane_credit: bool,
    revoked_reason: u8,
    now: u8,
    deadline: u8,
    accepted_mask: u8,
    close_count: [u8; 2],
    drop_count: [u8; 2],
    ack_count: [u8; 2],
    coverage: u32,
}

impl TxState {
    fn initial() -> Self {
        Self {
            current: 0,
            owners: [Owner::None; 2],
            phase: TxPhase::Empty,
            transfer_seq: 0,
            binding_generation: 0,
            ready_seq: [0; 2],
            pre_call_snapshot: [0; 2],
            runnable: false,
            worker_waiting: false,
            worker_epoch: 0,
            tls_lock: false,
            cancel_notify_pending: false,
            notify_pending: [false; 2],
            calls: 0,
            would_block_seen: false,
            result_kind: 0,
            result_cell: 0,
            result_identity: 0,
            result_binding: 0,
            result_transfer_seq: 0,
            completion_hint: false,
            poll_asleep: false,
            wake_token: false,
            timeout_armed: false,
            lane_credit: false,
            revoked_reason: 0,
            now: 0,
            deadline: 30,
            accepted_mask: 0,
            close_count: [0; 2],
            drop_count: [0; 2],
            ack_count: [0; 2],
            coverage: 0,
        }
    }

    fn index(self) -> usize {
        usize::from(self.current - 1)
    }

    fn has_unconsumed_readiness(self) -> bool {
        (0..2).any(|direction| self.ready_seq[direction] > self.pre_call_snapshot[direction])
    }
    fn clear_worker_readiness(&mut self) {
        // Per-direction history has no future observer after the worker returns or drops the stream.
        self.ready_seq = [0; 2];
        self.pre_call_snapshot = [0; 2];
    }

    fn successors(self) -> Vec<(Self, TxAction)> {
        let mut out = Vec::new();
        if self.current == 0 {
            let mut next = self;
            next.current = 1;
            next.transfer_seq = 1;
            next.binding_generation = 1;
            next.phase = TxPhase::Ready;
            next.owners[0] = Owner::Worker;
            next.lane_credit = true;
            out.push((next, TxAction::AdmitFirst));
        }
        if self.current != 0 {
            let index = self.index();
            if self.owners[index] == Owner::Worker
                && !self.tls_lock
                && matches!(
                    self.phase,
                    TxPhase::Ready
                        | TxPhase::Calling
                        | TxPhase::WouldBlock
                        | TxPhase::Waiting
                        | TxPhase::Runnable
                )
            {
                for direction in 0..2 {
                    if self.ready_seq[direction] < 2 {
                        let mut next = self;
                        next.ready_seq[direction] = bump(self.ready_seq[direction]);
                        next.worker_epoch = bump(self.worker_epoch);
                        next.notify_pending[direction] = true;
                        next.coverage |= if self.would_block_seen {
                            C_EDGE_AFTER_WB
                        } else {
                            C_EDGE_BEFORE_WB
                        };
                        out.push((
                            next,
                            if direction == 0 {
                                TxAction::PublishReadEdge
                            } else {
                                TxAction::PublishWriteEdge
                            },
                        ));
                    }
                }
            }
            if !self.tls_lock {
                for direction in 0..2 {
                    if self.notify_pending[direction] {
                        let mut next = self;
                        next.notify_pending[direction] = false;
                        if self.worker_waiting {
                            next.worker_waiting = false;
                            next.runnable = true;
                            next.phase = TxPhase::Runnable;
                            next.coverage |= C_WORKER_WOKE;
                        }
                        out.push((
                            next,
                            if direction == 0 {
                                TxAction::NotifyRead
                            } else {
                                TxAction::NotifyWrite
                            },
                        ));
                    }
                }
            }
            if self.owners[index] == Owner::Worker
                && self.revoked_reason == 0
                && self.calls < 3
                && matches!(self.phase, TxPhase::Ready | TxPhase::Runnable)
            {
                let mut next = self;
                next.phase = TxPhase::Calling;
                next.calls = bump(self.calls);
                next.pre_call_snapshot = self.ready_seq;
                next.runnable = false;
                next.worker_waiting = false;
                out.push((next, TxAction::WorkerCall));
            }
            if self.phase == TxPhase::Calling {
                if !self.would_block_seen {
                    let mut next = self;
                    next.phase = TxPhase::WouldBlock;
                    next.would_block_seen = true;
                    next.coverage |= C_WOULDBLOCK;
                    out.push((next, TxAction::NativeWouldBlock));
                }
                if self.calls < 3 {
                    let mut next = self;
                    next.phase = TxPhase::Runnable;
                    next.runnable = true;
                    next.coverage |= C_SYNTHETIC;
                    out.push((next, TxAction::NativeSyntheticLimit));
                }
                let mut success = self;
                success.clear_worker_readiness();
                success.phase = TxPhase::Returned;
                success.result_kind = 1;
                success.coverage |= C_SUCCESS;
                out.push((success, TxAction::NativeSuccess));
                let mut error = self;
                error.clear_worker_readiness();
                error.phase = TxPhase::Returned;
                error.result_kind = 2;
                error.coverage |= C_ERROR;
                out.push((error, TxAction::NativeError));
                let mut stuck = self;
                stuck.clear_worker_readiness();
                stuck.phase = TxPhase::Stuck;
                stuck.coverage |= C_STUCK;
                out.push((stuck, TxAction::NativeRemainsRunning));
            }
            if self.phase == TxPhase::WouldBlock && !self.tls_lock {
                let mut next = self;
                next.tls_lock = true;
                next.phase = TxPhase::LockedRecheck;
                out.push((next, TxAction::LockForRecheck));
            }
            if self.phase == TxPhase::LockedRecheck && self.tls_lock {
                let mut next = self;
                if self.revoked_reason != 0 || self.has_unconsumed_readiness() {
                    next.tls_lock = false;
                    next.phase = TxPhase::Runnable;
                    next.runnable = true;
                } else {
                    next.phase = TxPhase::NeedCondvarRelease;
                }
                out.push((next, TxAction::LockedRecheck));
            }
            if self.phase == TxPhase::NeedCondvarRelease && self.tls_lock {
                let mut next = self;
                next.tls_lock = false;
                next.worker_waiting = true;
                next.runnable = false;
                next.phase = TxPhase::Waiting;
                next.coverage |= C_WORKER_SLEPT;
                out.push((next, TxAction::AtomicCondvarRelease));
            }
            if self.cancel_notify_pending && !self.tls_lock {
                let mut next = self;
                next.cancel_notify_pending = false;
                if self.worker_waiting {
                    next.worker_waiting = false;
                    next.runnable = true;
                    next.phase = TxPhase::Runnable;
                    next.coverage |= C_WORKER_WOKE;
                }
                out.push((next, TxAction::CancellationWake));
            }
            if self.phase == TxPhase::Returned && !self.tls_lock {
                let mut next = self;
                next.tls_lock = true;
                out.push((next, TxAction::LockForResult));
            }
            if self.phase == TxPhase::Returned && self.tls_lock {
                let mut next = self;
                next.tls_lock = false;
                next.owners[index] = Owner::ResultCell;
                next.result_cell = self.current;
                next.result_identity = self.current;
                next.result_binding = self.binding_generation;
                next.result_transfer_seq = self.transfer_seq;
                next.completion_hint = true;
                next.phase = TxPhase::CellPublished;
                out.push((next, TxAction::PublishResultCell));
            }
            if self.completion_hint {
                let mut next = self;
                if self.wake_token {
                    next.coverage |= C_WAKE_COALESCED;
                }
                next.wake_token = true;
                out.push((next, TxAction::MioWake));
            }
            if !self.poll_asleep
                && self.result_cell == 0
                && !self.wake_token
                && !self.completion_hint
                && !(self.worker_waiting && self.has_unconsumed_readiness())
            {
                let mut next = self;
                next.poll_asleep = true;
                next.timeout_armed = true;
                out.push((next, TxAction::PollSleep));
            }
            if self.poll_asleep && self.wake_token {
                let mut next = self;
                next.poll_asleep = false;
                next.timeout_armed = false;
                out.push((next, TxAction::PollWake));
            }
            if self.poll_asleep && self.timeout_armed {
                let mut next = self;
                next.poll_asleep = false;
                next.timeout_armed = false;
                next.coverage |= C_POLL_TIMEOUT;
                out.push((next, TxAction::PollTimeout));
            }
            if !self.poll_asleep
                && self.result_cell == 0
                && (self.wake_token || self.completion_hint)
            {
                let mut next = self;
                next.wake_token = false;
                next.completion_hint = false;
                out.push((next, TxAction::PollDrain));
            }
            if !self.poll_asleep && self.result_cell != 0 {
                let mut next = self;
                let result_index = usize::from(self.result_cell - 1);
                next.owners[result_index] = Owner::Reactor;
                next.phase = TxPhase::Taken;
                // Cell consumption precedes clearing its completion hint and wake token.
                next.result_cell = 0;
                next.wake_token = false;
                next.completion_hint = false;
                out.push((next, TxAction::TakeResult));
            }
            if self.phase == TxPhase::Taken {
                let exact = self.result_identity == self.current
                    && self.result_binding == self.binding_generation
                    && self.result_transfer_seq == self.transfer_seq;
                if self.result_kind == 1
                    && self.revoked_reason == 0
                    && self.now < self.deadline
                    && exact
                {
                    let mut next = self;
                    next.phase = TxPhase::Accepted;
                    next.accepted_mask |= 1 << index;
                    next.lane_credit = false;
                    if next.accepted_mask == 3 {
                        next.coverage |= C_TWO_ACCEPTS;
                    }
                    out.push((next, TxAction::AcceptExactTransfer));
                } else {
                    let mut next = self;
                    next.owners[index] = Owner::Closed;
                    next.phase = TxPhase::Released;
                    next.lane_credit = false;
                    next.close_count[index] = 1;
                    next.drop_count[index] = 1;
                    next.ack_count[index] = 1;
                    out.push((next, TxAction::RejectAndClose));
                }
            }
            if self.owners[index] == Owner::Worker
                && self.revoked_reason != 0
                && !self.tls_lock
                && !matches!(
                    self.phase,
                    TxPhase::Calling | TxPhase::Waiting | TxPhase::Stuck
                )
            {
                let mut next = self;
                next.owners[index] = Owner::Closed;
                next.phase = TxPhase::Released;
                next.clear_worker_readiness();
                next.lane_credit = false;
                next.close_count[index] = 1;
                next.drop_count[index] = 1;
                next.ack_count[index] = 1;
                out.push((next, TxAction::WorkerCloseDrop));
            }
            if self.now == 0 {
                let mut next = self;
                next.now = 5;
                out.push((next, TxAction::AdvanceToFive));
            }
            if self.now == 5 {
                let mut next = self;
                next.now = self.deadline;
                out.push((next, TxAction::AdvanceToDeadline));
            }
            if self.revoked_reason == 0
                && self.phase != TxPhase::Accepted
                && self.phase != TxPhase::Released
                && self.phase != TxPhase::Stuck
                && self.now < self.deadline
            {
                let mut cancel = self;
                cancel.revoked_reason = 1;
                cancel.cancel_notify_pending = true;
                cancel.worker_epoch = bump(self.worker_epoch);
                cancel.coverage |= C_CANCEL;
                out.push((cancel, TxAction::RevokeCancel));
                if self.now == 5 {
                    let mut shutdown = self;
                    shutdown.revoked_reason = 3;
                    shutdown.cancel_notify_pending = true;
                    shutdown.worker_epoch = bump(self.worker_epoch);
                    shutdown.coverage |= C_SHUTDOWN;
                    out.push((shutdown, TxAction::RevokeShutdown));
                }
            }
            if self.revoked_reason == 0
                && self.phase != TxPhase::Accepted
                && self.phase != TxPhase::Released
                && self.phase != TxPhase::Stuck
                && self.now == self.deadline
            {
                let mut deadline = self;
                deadline.revoked_reason = 2;
                deadline.cancel_notify_pending = true;
                deadline.worker_epoch = bump(self.worker_epoch);
                deadline.coverage |= C_DEADLINE;
                out.push((deadline, TxAction::RevokeDeadline));
            }
            if self.current == 1
                && !self.lane_credit
                && self.result_cell == 0
                && matches!(self.phase, TxPhase::Accepted | TxPhase::Released)
            {
                let mut next = self;
                next.current = 2;
                next.transfer_seq = 2;
                next.binding_generation = 2;
                next.phase = TxPhase::Ready;
                next.owners[1] = Owner::Worker;
                next.ready_seq = [0; 2];
                next.pre_call_snapshot = [0; 2];
                next.runnable = false;
                next.worker_waiting = false;
                next.tls_lock = false;
                next.notify_pending = [false; 2];
                next.calls = 0;
                next.would_block_seen = false;
                next.result_kind = 0;
                next.result_identity = 0;
                next.result_binding = 0;
                next.result_transfer_seq = 0;
                next.revoked_reason = 0;
                next.now = 0;
                next.lane_credit = true;
                out.push((next, TxAction::AdmitSecondIdentity));
            }
        }
        out
    }

    fn assert_invariants(self) {
        if self.current == 0 {
            return;
        }
        for index in 0..2 {
            let admitted = self.owners[index] != Owner::None;
            if admitted {
                let result_matches = self.result_cell == (index as u8 + 1);
                assert_eq!(
                    self.owners[index] == Owner::ResultCell,
                    result_matches,
                    "one owner per stream and exact result-cell identity"
                );
            }
            if self.accepted_mask & (1 << index) != 0 {
                assert_eq!(self.owners[index], Owner::Reactor);
            }
        }
        if self.result_cell != 0 {
            assert!(
                self.completion_hint,
                "occupied result cell retains its completion hint"
            );
            assert!(!self.poll_asleep || self.wake_token || self.timeout_armed);
        }
        if self.worker_waiting && self.has_unconsumed_readiness() {
            let pending = self.notify_pending.iter().any(|pending| *pending);
            assert!(
                self.runnable || pending,
                "readiness cannot be lost while worker waits"
            );
        }
        let index = self.index();
        if !self.lane_credit {
            assert!(!matches!(
                self.owners[index],
                Owner::Worker | Owner::ResultCell
            ));
        }
        if self.owners[index] == Owner::Closed {
            assert_eq!(self.close_count[index], 1);
            assert_eq!(self.drop_count[index], 1);
            assert_eq!(self.ack_count[index], 1, "ack follows actual close/drop");
        }
        if self.revoked_reason != 0 {
            assert_ne!(self.accepted_mask & (1 << index), 1 << index);
        }
        if self.phase == TxPhase::Stuck {
            assert!(
                self.lane_credit,
                "a stuck native call keeps physical lane custody"
            );
            assert_eq!(self.owners[index], Owner::Worker);
        }
    }
}

#[test]
fn g2_i_exhaustive_worker_reactor_producer_transfer_model() {
    let start = TxState::initial();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut coverage = 0_u32;
    let mut start_key = start;
    start_key.worker_epoch = 0;
    seen.insert(start_key);
    queue.push_back(start);
    while let Some(state) = queue.pop_front() {
        state.assert_invariants();
        for (mut next, _) in state.successors() {
            // Coverage is transition history, not scheduler state.
            coverage |= next.coverage;
            next.coverage = 0;
            assert!(
                next.worker_epoch <= MAX_WORKER_EPOCH,
                "worker epoch respects the finite event bound without wrapping"
            );
            // The checked epoch is history-only: readiness snapshots, pending notifications,
            // runnable, and sleeping state determine every future transition.
            let mut key = next;
            key.worker_epoch = 0;
            if seen.insert(key) {
                assert!(seen.len() < 500_000, "G2-I transfer state bound exceeded");
                queue.push_back(next);
            }
        }
    }
    let required = C_WOULDBLOCK
        | C_EDGE_BEFORE_WB
        | C_EDGE_AFTER_WB
        | C_WORKER_SLEPT
        | C_WORKER_WOKE
        | C_SYNTHETIC
        | C_SUCCESS
        | C_ERROR
        | C_STUCK
        | C_POLL_TIMEOUT
        | C_WAKE_COALESCED
        | C_TWO_ACCEPTS
        | C_CANCEL
        | C_DEADLINE
        | C_SHUTDOWN;
    assert_eq!(
        coverage & required,
        required,
        "all required transfer schedules are reachable"
    );
    println!(
        "G2-I transfer_model explored_states={} worker_epoch_upper_bound={}",
        seen.len(),
        MAX_WORKER_EPOCH
    );
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CapAction {
    WaiterLockK2,
    RegisterMarkerUnderK2,
    RecheckLaneUnderK2,
    AtomicCondvarRelease,
    AcceptTransferRelease,
    ActualCloseDrop,
    PublishLaneRelease,
    ReactorLockK2,
    ReadMarkerAndUnlock,
    NotifyWaiter,
    WaiterWakeAndReacquire,
    AdvanceToFive,
    AdvanceToTwentyFive,
    AdvanceToTwentyNine,
    AdvanceToDeadline,
    WssStartAtDeadlineFloor,
    TypedInsufficientRemainingTime,
    WssCancel,
    WssExpireAtD,
}

const CAP_RELEASE_BEFORE_REGISTER: u16 = 1 << 0;
const CAP_RELEASE_AFTER_RECHECK: u16 = 1 << 1;
const CAP_WAITER_SLEPT: u16 = 1 << 2;
const CAP_WAITER_WOKE: u16 = 1 << 3;
const CAP_STARTED_AT_FLOOR: u16 = 1 << 4;
const CAP_TYPED_REFUSAL: u16 = 1 << 5;
const CAP_EXPIRED_ONCE: u16 = 1 << 6;
const CAP_CANCEL_WON: u16 = 1 << 7;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CapacityState {
    now: u8,
    deadline: u8,
    min_remaining: u8,
    lane_free: bool,
    release_source: u8,
    release_published: bool,
    release_pending: bool,
    k2_locked: bool,
    k2_owner: u8,
    marker_registered: bool,
    waiter_phase: u8,
    waiter_sleeping: bool,
    wake_pending: bool,
    notify_delivered: bool,
    epoch: u8,
    wss_waiting: bool,
    wss_started: bool,
    wss_started_at: u8,
    wss_refused: bool,
    wss_cancelled: bool,
    wss_expired: bool,
    charged_start: bool,
    socket_created: bool,
    spacing_advanced: bool,
    ttl_refreshed: bool,
    coverage: u16,
}

impl CapacityState {
    fn initial() -> Self {
        Self {
            now: 0,
            deadline: 30,
            min_remaining: 5,
            lane_free: false,
            release_source: 0,
            release_published: false,
            release_pending: false,
            k2_locked: false,
            k2_owner: 0,
            marker_registered: false,
            waiter_phase: 0,
            waiter_sleeping: false,
            wake_pending: false,
            notify_delivered: false,
            epoch: 0,
            wss_waiting: true,
            wss_started: false,
            wss_started_at: 0,
            wss_refused: false,
            wss_cancelled: false,
            wss_expired: false,
            charged_start: false,
            socket_created: false,
            spacing_advanced: false,
            ttl_refreshed: false,
            coverage: 0,
        }
    }

    fn successors(self) -> Vec<(Self, CapAction)> {
        let mut out = Vec::new();
        if self.now == 0 {
            let mut next = self;
            next.now = 5;
            out.push((next, CapAction::AdvanceToFive));
        }
        if self.now == 5 {
            let mut next = self;
            next.now = 25;
            out.push((next, CapAction::AdvanceToTwentyFive));
        }
        if self.now == 25 {
            let mut next = self;
            next.now = 29;
            out.push((next, CapAction::AdvanceToTwentyNine));
        }
        if self.now == 29 {
            let mut next = self;
            next.now = self.deadline;
            out.push((next, CapAction::AdvanceToDeadline));
        }
        if self.release_source == 0 {
            let mut accepted = self;
            accepted.release_source = 1;
            out.push((accepted, CapAction::AcceptTransferRelease));
            let mut dropped = self;
            dropped.release_source = 2;
            out.push((dropped, CapAction::ActualCloseDrop));
        }
        if self.release_source != 0 && !self.release_published {
            let mut next = self;
            next.release_published = true;
            next.release_pending = true;
            next.lane_free = true;
            if next.waiter_phase == 0 {
                next.coverage |= CAP_RELEASE_BEFORE_REGISTER;
            }
            if next.waiter_phase == 3 || next.waiter_phase == 5 {
                next.coverage |= CAP_RELEASE_AFTER_RECHECK;
            }
            out.push((next, CapAction::PublishLaneRelease));
        }
        if self.waiter_phase == 0 && self.wss_waiting && self.k2_owner == 0 {
            let mut next = self;
            next.k2_locked = true;
            next.k2_owner = 1;
            next.waiter_phase = 1;
            out.push((next, CapAction::WaiterLockK2));
        }
        if self.waiter_phase == 1 && self.k2_owner == 1 {
            let mut next = self;
            next.marker_registered = true;
            next.waiter_phase = 2;
            out.push((next, CapAction::RegisterMarkerUnderK2));
        }
        if self.waiter_phase == 2 && self.k2_owner == 1 {
            let mut next = self;
            if self.lane_free {
                next.k2_locked = false;
                next.k2_owner = 0;
                next.marker_registered = false;
                next.waiter_phase = 4;
            } else {
                next.waiter_phase = 3;
            }
            out.push((next, CapAction::RecheckLaneUnderK2));
        }
        if self.waiter_phase == 3 && self.k2_owner == 1 {
            let mut next = self;
            next.k2_locked = false;
            next.k2_owner = 0;
            next.waiter_sleeping = true;
            next.waiter_phase = 5;
            next.coverage |= CAP_WAITER_SLEPT;
            out.push((next, CapAction::AtomicCondvarRelease));
        }
        if self.release_pending && self.k2_owner == 0 {
            let mut next = self;
            next.k2_locked = true;
            next.k2_owner = 2;
            out.push((next, CapAction::ReactorLockK2));
        }
        if self.release_pending && self.k2_owner == 2 {
            let mut next = self;
            next.release_pending = false;
            if self.marker_registered {
                next.epoch = bump(self.epoch);
                next.wake_pending = true;
            }
            next.k2_locked = false;
            next.k2_owner = 0;
            out.push((next, CapAction::ReadMarkerAndUnlock));
        }
        if self.wake_pending {
            let mut next = self;
            next.wake_pending = false;
            if self.waiter_sleeping {
                next.notify_delivered = true;
            }
            out.push((next, CapAction::NotifyWaiter));
        }
        if self.waiter_sleeping && self.notify_delivered && self.k2_owner == 0 {
            let mut next = self;
            next.notify_delivered = false;
            next.waiter_sleeping = false;
            next.k2_locked = true;
            next.k2_owner = 1;
            next.waiter_phase = 2;
            next.coverage |= CAP_WAITER_WOKE;
            out.push((next, CapAction::WaiterWakeAndReacquire));
        }
        if self.waiter_phase == 4 && self.lane_free && self.wss_waiting && !self.wss_cancelled {
            let remaining = self.deadline - self.now;
            if self.now < self.deadline && remaining >= self.min_remaining {
                let mut next = self;
                next.wss_started = true;
                next.wss_started_at = self.now;
                next.wss_waiting = false;
                next.charged_start = true;
                next.socket_created = true;
                next.spacing_advanced = true;
                next.ttl_refreshed = true;
                if remaining == self.min_remaining {
                    next.coverage |= CAP_STARTED_AT_FLOOR;
                }
                out.push((next, CapAction::WssStartAtDeadlineFloor));
            } else if self.now < self.deadline {
                let mut next = self;
                next.wss_refused = true;
                next.waiter_phase = 6;
                next.coverage |= CAP_TYPED_REFUSAL;
                out.push((next, CapAction::TypedInsufficientRemainingTime));
            }
        }
        if self.now < self.deadline && self.wss_waiting && !self.wss_cancelled {
            let mut next = self;
            next.wss_cancelled = true;
            next.wss_waiting = false;
            next.coverage |= CAP_CANCEL_WON;
            out.push((next, CapAction::WssCancel));
        }
        if self.now == self.deadline && self.wss_waiting && !self.wss_cancelled && !self.wss_expired
        {
            let mut next = self;
            next.wss_expired = true;
            next.wss_waiting = false;
            next.coverage |= CAP_EXPIRED_ONCE;
            out.push((next, CapAction::WssExpireAtD));
        }
        out
    }

    fn assert_invariants(self) {
        assert_eq!(self.deadline, 30, "capacity deadline is immutable");
        assert_eq!(self.min_remaining, 5);
        assert_eq!(self.k2_locked, self.k2_owner != 0);
        if self.waiter_sleeping && self.lane_free {
            assert!(self.wake_pending || self.notify_delivered || self.release_pending);
        }
        if self.wss_refused {
            assert!(!self.charged_start);
            assert!(!self.socket_created);
            assert!(!self.spacing_advanced);
            assert!(!self.ttl_refreshed);
            assert_eq!(
                self.waiter_phase, 6,
                "insufficient-time demand is ineligible"
            );
        }
        if self.wss_started {
            assert!(self.wss_started_at < self.deadline);
            assert!(self.deadline - self.wss_started_at >= self.min_remaining);
            assert!(self.charged_start && self.socket_created);
        }
        if self.wss_expired {
            assert_eq!(self.now, self.deadline);
            assert!(!self.wss_waiting);
        }
        if self.wss_cancelled {
            assert!(
                !self.wss_expired,
                "stronger cancellation wins over waiter deadline"
            );
        }
    }
}

#[test]
fn g2_i_capacity_waiter_register_recheck_sleep_model() {
    let start = CapacityState::initial();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut coverage = 0_u16;
    seen.insert(start);
    queue.push_back(start);
    while let Some(state) = queue.pop_front() {
        state.assert_invariants();
        for (mut next, _) in state.successors() {
            // Coverage is transition history, not scheduler state.
            coverage |= next.coverage;
            next.coverage = 0;
            if seen.insert(next) {
                assert!(seen.len() < 100_000, "G2-I capacity state bound exceeded");
                queue.push_back(next);
            }
        }
    }
    let required = CAP_RELEASE_BEFORE_REGISTER
        | CAP_RELEASE_AFTER_RECHECK
        | CAP_WAITER_SLEPT
        | CAP_WAITER_WOKE
        | CAP_STARTED_AT_FLOOR
        | CAP_TYPED_REFUSAL
        | CAP_EXPIRED_ONCE
        | CAP_CANCEL_WON;
    assert_eq!(
        coverage & required,
        required,
        "all K2 capacity/deadline schedules are reachable"
    );
    println!("G2-I capacity_model explored_states={}", seen.len());
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RevocationReason {
    None,
    Stop,
    Logout,
    WithdrawLogin,
    SwitchWorld,
    IdentityChanged,
    FenceAbort,
    GenericAbort,
    Deadline,
    ProcessShutdown,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ProofAction {
    AdvanceToFive,
    AdvanceToDeadline,
    Revoke(RevocationReason),
    NativeReturn,
    ConsumeExactReceipt,
    DuplicateReceiptDelivery,
    IgnoreDuplicateReceipt,
    AcceptTransfer,
    CloseDropA,
    AckA,
    StartSameSlotTcpA2,
    ReturnA2,
    CloseDropA2,
    AckA2,
    QueueC,
    TryGrantC,
}

const P_EARLY_TCP: u16 = 1 << 0;
const P_PROOF_ON_SHUTDOWN: u16 = 1 << 1;
const P_STALE_RECEIPT_IGNORED: u16 = 1 << 2;
const P_A2_HELD_AFTER_OLD_ACK: u16 = 1 << 3;
const P_C_AFTER_A2_ACK: u16 = 1 << 4;
const P_ACCEPT_PATH: u16 = 1 << 5;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ProofState {
    now: u8,
    deadline: u8,
    reason: RevocationReason,
    revoked_at: u8,
    accepted: bool,
    worker_running: bool,
    native_returned: bool,
    owner_a: Owner,
    lane_credit: bool,
    queue_flight_a: bool,
    queue_lease_a: bool,
    proof_published: bool,
    proof_id: u8,
    proof_transfer_seq: u8,
    proof_time: u8,
    proof_consumed: bool,
    duplicate_receipt: bool,
    queue_settle_count: u8,
    lease_release_count: u8,
    admission_open: bool,
    tcp_demand: bool,
    tcp_a2_started: bool,
    a2_queue_flight: bool,
    a2_retiring: bool,
    a2_close_drop: bool,
    a2_retirement_ack: u8,
    a_close_drop: bool,
    a_retirement_ack: u8,
    c_demand: bool,
    c_granted: bool,
    reason_mask: u16,
    coverage: u16,
}

impl ProofState {
    fn initial() -> Self {
        Self {
            now: 0,
            deadline: 30,
            reason: RevocationReason::None,
            revoked_at: 0,
            accepted: false,
            worker_running: true,
            native_returned: false,
            owner_a: Owner::Worker,
            lane_credit: true,
            queue_flight_a: true,
            queue_lease_a: true,
            proof_published: false,
            proof_id: 0,
            proof_transfer_seq: 0,
            proof_time: 0,
            proof_consumed: false,
            duplicate_receipt: false,
            queue_settle_count: 0,
            lease_release_count: 0,
            admission_open: true,
            tcp_demand: true,
            tcp_a2_started: false,
            a2_queue_flight: false,
            a2_retiring: false,
            a2_close_drop: false,
            a2_retirement_ack: 0,
            a_close_drop: false,
            a_retirement_ack: 0,
            c_demand: false,
            c_granted: false,
            reason_mask: 0,
            coverage: 0,
        }
    }

    fn successors(self) -> Vec<(Self, ProofAction)> {
        let mut out = Vec::new();
        if self.now == 0 {
            let mut next = self;
            next.now = 5;
            out.push((next, ProofAction::AdvanceToFive));
        }
        if self.now == 5 {
            let mut next = self;
            next.now = self.deadline;
            out.push((next, ProofAction::AdvanceToDeadline));
        }
        if self.reason == RevocationReason::None && !self.accepted {
            let reasons = [
                RevocationReason::Stop,
                RevocationReason::Logout,
                RevocationReason::WithdrawLogin,
                RevocationReason::SwitchWorld,
                RevocationReason::IdentityChanged,
                RevocationReason::FenceAbort,
                RevocationReason::GenericAbort,
                RevocationReason::Deadline,
                RevocationReason::ProcessShutdown,
            ];
            for reason in reasons {
                let time_ok = if reason == RevocationReason::Deadline {
                    self.now == self.deadline
                } else {
                    self.now == 5
                };
                if time_ok {
                    let mut next = self;
                    next.reason = reason;
                    next.revoked_at = self.now;
                    next.proof_published = true;
                    next.proof_id = 1;
                    next.proof_transfer_seq = 1;
                    next.proof_time = self.now;
                    next.reason_mask |= 1 << (reason as u8 - 1);
                    if reason == RevocationReason::ProcessShutdown {
                        next.admission_open = false;
                        next.coverage |= P_PROOF_ON_SHUTDOWN;
                    }
                    out.push((next, ProofAction::Revoke(reason)));
                }
            }
        }
        if self.worker_running {
            let mut next = self;
            next.worker_running = false;
            next.native_returned = true;
            out.push((next, ProofAction::NativeReturn));
        }
        if self.proof_published
            && !self.proof_consumed
            && self.proof_id == 1
            && self.proof_transfer_seq == 1
            && !self.accepted
        {
            let mut next = self;
            next.proof_consumed = true;
            next.queue_flight_a = false;
            next.queue_lease_a = false;
            next.queue_settle_count = 1;
            next.lease_release_count = 1;
            out.push((next, ProofAction::ConsumeExactReceipt));
        }
        if self.proof_consumed && !self.duplicate_receipt {
            let mut next = self;
            next.duplicate_receipt = true;
            out.push((next, ProofAction::DuplicateReceiptDelivery));
        }
        if self.duplicate_receipt {
            let mut next = self;
            next.duplicate_receipt = false;
            next.coverage |= P_STALE_RECEIPT_IGNORED;
            out.push((next, ProofAction::IgnoreDuplicateReceipt));
        }
        if self.native_returned
            && self.reason == RevocationReason::None
            && !self.proof_published
            && !self.accepted
            && self.now < self.deadline
        {
            let mut next = self;
            next.accepted = true;
            next.owner_a = Owner::Reactor;
            next.lane_credit = false;
            next.coverage |= P_ACCEPT_PATH;
            out.push((next, ProofAction::AcceptTransfer));
        }
        if self.native_returned
            && self.reason != RevocationReason::None
            && !self.a_close_drop
            && !self.accepted
        {
            let mut next = self;
            next.a_close_drop = true;
            next.owner_a = Owner::Closed;
            next.lane_credit = false;
            out.push((next, ProofAction::CloseDropA));
        }
        if self.a_close_drop && self.a_retirement_ack == 0 {
            let mut next = self;
            next.a_retirement_ack = 1;
            if self.a2_retiring && self.a2_retirement_ack == 0 {
                next.coverage |= P_A2_HELD_AFTER_OLD_ACK;
            }
            out.push((next, ProofAction::AckA));
        }
        if self.proof_consumed
            && self.tcp_demand
            && self.admission_open
            && !self.tcp_a2_started
            && !self.queue_flight_a
            && !self.queue_lease_a
        {
            let mut next = self;
            next.tcp_a2_started = true;
            next.a2_queue_flight = true;
            if self.worker_running && self.lane_credit {
                next.coverage |= P_EARLY_TCP;
            }
            out.push((next, ProofAction::StartSameSlotTcpA2));
        }
        if self.tcp_a2_started && self.a2_queue_flight && !self.a2_retiring {
            let mut next = self;
            next.a2_queue_flight = false;
            next.a2_retiring = true;
            out.push((next, ProofAction::ReturnA2));
        }
        if self.a2_retiring && !self.a2_close_drop {
            let mut next = self;
            next.a2_close_drop = true;
            out.push((next, ProofAction::CloseDropA2));
        }
        if self.a2_close_drop && self.a2_retirement_ack == 0 {
            let mut next = self;
            next.a2_retirement_ack = 1;
            out.push((next, ProofAction::AckA2));
        }
        if self.a2_retiring && !self.c_demand {
            let mut next = self;
            next.c_demand = true;
            out.push((next, ProofAction::QueueC));
        }
        if self.c_demand && !self.c_granted && self.a2_retiring && self.a2_retirement_ack == 1 {
            let mut next = self;
            next.c_granted = true;
            next.coverage |= P_C_AFTER_A2_ACK;
            out.push((next, ProofAction::TryGrantC));
        }
        out
    }

    fn assert_invariants(self) {
        assert_eq!(self.deadline, 30);
        if self.reason != RevocationReason::None {
            assert!(
                self.proof_published,
                "every never-Accepted revocation publishes proof immediately"
            );
            assert_eq!(self.proof_id, 1);
            assert_eq!(self.proof_transfer_seq, 1);
            assert_eq!(self.proof_time, self.revoked_at);
            assert!(!self.accepted);
        }
        if self.proof_consumed {
            assert!(self.proof_published);
            assert!(!self.queue_flight_a);
            assert!(!self.queue_lease_a);
            assert_eq!(self.queue_settle_count, 1);
            assert_eq!(self.lease_release_count, 1);
            assert!(
                !self.accepted,
                "consumed initial-TLS proof forbids Accepted"
            );
        }
        if self.lane_credit {
            assert!(
                self.worker_running
                    || (self.native_returned && !self.a_close_drop && !self.accepted)
            );
        }
        if self.accepted {
            assert!(!self.lane_credit);
            assert_eq!(self.owner_a, Owner::Reactor);
            assert!(!self.proof_published);
        }
        if self.a_close_drop {
            assert!(!self.worker_running);
            assert_eq!(self.owner_a, Owner::Closed);
        }
        if self.a_retirement_ack != 0 {
            assert!(
                self.a_close_drop,
                "physical acknowledgement follows A close/drop"
            );
        }
        if self.a2_retiring && self.a2_retirement_ack == 0 && self.c_granted {
            panic!("old A acknowledgement cannot release A2 retirement cell");
        }
        if self.c_granted {
            assert!(
                self.c_demand && self.a2_retiring && self.a2_retirement_ack == 1,
                "C's retirement blocker is released only by A2's physical acknowledgement"
            );
        }
        if self.a2_retirement_ack != 0 {
            assert!(self.a2_close_drop);
        }
    }
}

#[test]
fn g2_i_initial_tls_proof_queue_release_and_exact_retirement_model() {
    let start = ProofState::initial();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut reason_mask = 0_u16;
    let mut coverage = 0_u16;
    seen.insert(start);
    queue.push_back(start);
    while let Some(state) = queue.pop_front() {
        state.assert_invariants();
        for (mut next, _) in state.successors() {
            // Coverage and reason masks summarize explored transitions, not scheduler state.
            reason_mask |= next.reason_mask;
            coverage |= next.coverage;
            next.reason_mask = 0;
            next.coverage = 0;
            if seen.insert(next) {
                assert!(
                    seen.len() < 200_000,
                    "G2-I proof/retirement state bound exceeded"
                );
                queue.push_back(next);
            }
        }
    }
    assert_eq!(
        reason_mask,
        (1 << 9) - 1,
        "runs every revocation reason, including shutdown"
    );
    let required = P_EARLY_TCP
        | P_PROOF_ON_SHUTDOWN
        | P_STALE_RECEIPT_IGNORED
        | P_A2_HELD_AFTER_OLD_ACK
        | P_C_AFTER_A2_ACK
        | P_ACCEPT_PATH;
    assert_eq!(coverage & required, required);
    println!("G2-I proof_retirement_model explored_states={}", seen.len());
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ClearAction {
    Claim,
    NativeWouldBlock,
    ReactorEdge,
    WrongClearWithoutLockedRecheck,
    Sleep,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ClearMutant {
    stage: u8,
    snapshot: u8,
    ready_seq: u8,
    waiting: bool,
    runnable: bool,
    notification_pending: bool,
}

impl ClearMutant {
    fn successors(self) -> Vec<(Self, ClearAction)> {
        let mut out = Vec::new();
        if self.stage == 0 {
            let mut next = self;
            next.stage = 1;
            next.snapshot = self.ready_seq;
            out.push((next, ClearAction::Claim));
        }
        if self.stage == 1 {
            let mut next = self;
            next.stage = 2;
            out.push((next, ClearAction::NativeWouldBlock));
        }
        if self.stage == 2 && self.ready_seq == 0 {
            let mut next = self;
            next.ready_seq = 1;
            next.notification_pending = false;
            out.push((next, ClearAction::ReactorEdge));
        }
        if self.stage == 2 && self.ready_seq != 0 {
            let mut next = self;
            next.runnable = false;
            next.stage = 3;
            out.push((next, ClearAction::WrongClearWithoutLockedRecheck));
        }
        if self.stage == 3 {
            let mut next = self;
            next.waiting = true;
            next.stage = 4;
            out.push((next, ClearAction::Sleep));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CompletionAction {
    NativeReturn,
    MioWake,
    PollReturn,
    PublishResultAfterWake,
    PublishResultBeforeWake,
    ClearHintWithoutReadingCell,
    PollSleepWithoutTimeoutDrain,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CompletionMutant {
    returned: bool,
    cell_occupied: bool,
    hint: bool,
    wake_token: bool,
    poll_asleep: bool,
    timeout_drain_enabled: bool,
    poll_returned: bool,
    late_published: bool,
}

impl CompletionMutant {
    fn successors(self) -> Vec<(Self, CompletionAction)> {
        let mut out = Vec::new();
        if !self.returned {
            let mut next = self;
            next.returned = true;
            out.push((next, CompletionAction::NativeReturn));
        }
        if self.returned && !self.wake_token {
            let mut next = self;
            next.wake_token = true;
            out.push((next, CompletionAction::MioWake));
        }
        if self.poll_asleep && self.wake_token {
            let mut next = self;
            next.poll_asleep = false;
            next.wake_token = false;
            next.poll_returned = true;
            out.push((next, CompletionAction::PollReturn));
        }
        if self.returned && !self.cell_occupied && self.poll_returned {
            let mut next = self;
            next.cell_occupied = true;
            next.hint = true;
            next.late_published = true;
            out.push((next, CompletionAction::PublishResultAfterWake));
        }
        if self.returned && !self.cell_occupied {
            let mut next = self;
            next.cell_occupied = true;
            next.hint = true;
            out.push((next, CompletionAction::PublishResultBeforeWake));
        }
        if self.cell_occupied && self.hint && self.poll_returned {
            let mut next = self;
            next.hint = false;
            out.push((next, CompletionAction::ClearHintWithoutReadingCell));
        }
        if !self.poll_asleep && !self.timeout_drain_enabled {
            let mut next = self;
            next.poll_asleep = true;
            out.push((next, CompletionAction::PollSleepWithoutTimeoutDrain));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum IdentityAction {
    CompleteIdentityA,
    RetireIdentityA,
    AuthorizeIdentityB,
    DeliverOldResultWithoutBindingCheck,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct IdentityMutant {
    old_result_pending: bool,
    old_retired: bool,
    current_binding: u8,
    result_binding: u8,
    installed_binding: u8,
}

impl IdentityMutant {
    fn successors(self) -> Vec<(Self, IdentityAction)> {
        let mut out = Vec::new();
        if !self.old_result_pending {
            let mut next = self;
            next.old_result_pending = true;
            next.result_binding = 1;
            out.push((next, IdentityAction::CompleteIdentityA));
        }
        if self.old_result_pending && !self.old_retired {
            let mut next = self;
            next.old_retired = true;
            out.push((next, IdentityAction::RetireIdentityA));
        }
        if self.old_retired && self.current_binding == 1 {
            let mut next = self;
            next.current_binding = 2;
            out.push((next, IdentityAction::AuthorizeIdentityB));
        }
        if self.old_result_pending && self.current_binding == 2 && self.installed_binding == 0 {
            let mut next = self;
            // The mutant omits exact binding_generation and transfer_seq checks.
            next.installed_binding = self.result_binding;
            out.push((next, IdentityAction::DeliverOldResultWithoutBindingCheck));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum CapacityMutantAction {
    CheckLaneOutsideK2,
    PublishReleaseAndCheckUnarmedMarker,
    RegisterAfterRelease,
    SleepWithoutLaneRecheck,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CapacityMutant {
    lane_free: bool,
    checked_busy_outside_k2: bool,
    marker_registered: bool,
    lost_wake: bool,
    sleeping: bool,
}

impl CapacityMutant {
    fn successors(self) -> Vec<(Self, CapacityMutantAction)> {
        let mut out = Vec::new();
        if !self.checked_busy_outside_k2 {
            let mut next = self;
            next.checked_busy_outside_k2 = true;
            out.push((next, CapacityMutantAction::CheckLaneOutsideK2));
        }
        if self.checked_busy_outside_k2 && !self.lane_free {
            let mut next = self;
            next.lane_free = true;
            next.lost_wake = !self.marker_registered;
            out.push((
                next,
                CapacityMutantAction::PublishReleaseAndCheckUnarmedMarker,
            ));
        }
        if self.lane_free && !self.marker_registered {
            let mut next = self;
            next.marker_registered = true;
            out.push((next, CapacityMutantAction::RegisterAfterRelease));
        }
        if self.marker_registered && self.lane_free && self.lost_wake && !self.sleeping {
            let mut next = self;
            next.sleeping = true;
            out.push((next, CapacityMutantAction::SleepWithoutLaneRecheck));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum DeadlineMutantAction {
    AdvanceToD,
    AttemptQueueReductionAtD,
    ReleasePhysicalLaneAtD,
    AdmitReplacementHandshake,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DeadlineMutant {
    now: u8,
    worker_running: bool,
    owner_count: u8,
    lane_credit: bool,
    replacement_admitted: bool,
    queue_credit: bool,
    tcp_waiting: bool,
    reduction_attempted: bool,
}

impl DeadlineMutant {
    fn successors(self) -> Vec<(Self, DeadlineMutantAction)> {
        let mut out = Vec::new();
        if self.now < 30 {
            let mut next = self;
            next.now = 30;
            out.push((next, DeadlineMutantAction::AdvanceToD));
        }
        if self.now == 30
            && self.worker_running
            && self.queue_credit
            && self.tcp_waiting
            && !self.reduction_attempted
        {
            let mut next = self;
            next.reduction_attempted = true;
            out.push((next, DeadlineMutantAction::AttemptQueueReductionAtD));
        }
        if self.now == 30 && self.worker_running && self.lane_credit {
            let mut next = self;
            next.lane_credit = false;
            out.push((next, DeadlineMutantAction::ReleasePhysicalLaneAtD));
        }
        if self.now == 30 && !self.lane_credit && !self.replacement_admitted {
            let mut next = self;
            next.replacement_admitted = true;
            next.owner_count = bump(self.owner_count);
            out.push((next, DeadlineMutantAction::AdmitReplacementHandshake));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum PreDeadlineMutantAction {
    AdvanceToFive,
    CancelAndPublishReceiptAtFive,
    WronglyDeferReceiptConsumptionUntilD,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreDeadlineMutant {
    now: u8,
    revoked: bool,
    receipt_published: bool,
    receipt_time: u8,
    receipt_consumed: bool,
    reduction_attempted: bool,
    queue_credit: bool,
    worker_running: bool,
    tcp_waiting: bool,
}

impl PreDeadlineMutant {
    fn successors(self) -> Vec<(Self, PreDeadlineMutantAction)> {
        let mut out = Vec::new();
        if self.now == 0 {
            let mut next = self;
            next.now = 5;
            out.push((next, PreDeadlineMutantAction::AdvanceToFive));
        }
        if self.now == 5 && !self.revoked {
            let mut next = self;
            next.revoked = true;
            next.receipt_published = true;
            next.receipt_time = 5;
            out.push((next, PreDeadlineMutantAction::CancelAndPublishReceiptAtFive));
        }
        if self.revoked
            && self.receipt_published
            && !self.receipt_consumed
            && !self.reduction_attempted
        {
            let mut next = self;
            // This mutant keeps queue admission until D or native return.
            next.reduction_attempted = true;
            out.push((
                next,
                PreDeadlineMutantAction::WronglyDeferReceiptConsumptionUntilD,
            ));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ProofMutantAction {
    PublishReceiptForA,
    ReleaseAQueue,
    AdmitSameSlotA2,
    DeliverStaleReceiptToA2,
    RevokeA,
    ReturnAWithSuccess,
    AcceptAfterProof,
    ReturnA2,
    CloseA2,
    CloseA,
    AckABySlotOnly,
    GrantC,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ProofMutant {
    receipt_a_published: bool,
    queue_a_released: bool,
    a2_admitted: bool,
    a2_queue_released: bool,
    stale_receipt_consumed: bool,
    a_revoked: bool,
    a_success_returned: bool,
    a_accepted: bool,
    a2_retiring: bool,
    a_closed: bool,
    a2_closed: bool,
    ack_a: bool,
    ack_a2: bool,
    c_granted: bool,
}

impl ProofMutant {
    fn successors(self) -> Vec<(Self, ProofMutantAction)> {
        let mut out = Vec::new();
        if !self.receipt_a_published {
            let mut next = self;
            next.receipt_a_published = true;
            out.push((next, ProofMutantAction::PublishReceiptForA));
        }
        if self.receipt_a_published && !self.queue_a_released {
            let mut next = self;
            next.queue_a_released = true;
            out.push((next, ProofMutantAction::ReleaseAQueue));
        }
        if self.queue_a_released && !self.a2_admitted {
            let mut next = self;
            next.a2_admitted = true;
            out.push((next, ProofMutantAction::AdmitSameSlotA2));
        }
        if self.a2_admitted && self.receipt_a_published && !self.a2_queue_released {
            let mut next = self;
            next.a2_queue_released = true;
            next.stale_receipt_consumed = true;
            out.push((next, ProofMutantAction::DeliverStaleReceiptToA2));
        }
        if !self.a_revoked {
            let mut next = self;
            next.a_revoked = true;
            next.receipt_a_published = true;
            out.push((next, ProofMutantAction::RevokeA));
        }
        if self.a_revoked && !self.a_success_returned {
            let mut next = self;
            next.a_success_returned = true;
            out.push((next, ProofMutantAction::ReturnAWithSuccess));
        }
        if self.a_success_returned && !self.a_accepted {
            let mut next = self;
            next.a_accepted = true;
            out.push((next, ProofMutantAction::AcceptAfterProof));
        }
        if self.a2_admitted && !self.a2_retiring {
            let mut next = self;
            next.a2_retiring = true;
            out.push((next, ProofMutantAction::ReturnA2));
        }
        if self.a2_retiring && !self.a2_closed {
            let mut next = self;
            next.a2_closed = true;
            out.push((next, ProofMutantAction::CloseA2));
        }
        if self.a2_retiring && !self.a_closed {
            let mut next = self;
            next.a_closed = true;
            out.push((next, ProofMutantAction::CloseA));
        }
        if self.a_closed && self.a2_retiring && !self.ack_a2 {
            let mut next = self;
            next.ack_a = true;
            next.ack_a2 = true;
            out.push((next, ProofMutantAction::AckABySlotOnly));
        }
        if self.a2_retiring && self.ack_a2 && !self.c_granted {
            let mut next = self;
            next.c_granted = true;
            out.push((next, ProofMutantAction::GrantC));
        }
        if self.a2_retiring && self.a2_closed && !self.ack_a2 {
            let mut next = self;
            next.ack_a2 = true;
            out.push((next, ProofMutantAction::GrantC));
        }
        out
    }
}

#[test]
fn g2_i_mandatory_mutants_have_shortest_counterexamples() {
    let (wrong_clear, clear_states) = shortest_counterexample(
        ClearMutant {
            stage: 0,
            snapshot: 0,
            ready_seq: 0,
            waiting: false,
            runnable: false,
            notification_pending: false,
        },
        |state| state.successors(),
        |state| {
            state.waiting
                && state.ready_seq > state.snapshot
                && !state.runnable
                && !state.notification_pending
        },
        |action| format!("{action:?}"),
        100,
    );
    let expected_wrong_clear = vec![
        "Claim",
        "NativeWouldBlock",
        "ReactorEdge",
        "WrongClearWithoutLockedRecheck",
        "Sleep",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    assert_eq!(
        wrong_clear, expected_wrong_clear,
        "shortest wrong-clear schedule"
    );

    let comp_start = CompletionMutant {
        returned: false,
        cell_occupied: false,
        hint: false,
        wake_token: false,
        poll_asleep: true,
        timeout_drain_enabled: false,
        poll_returned: false,
        late_published: false,
    };
    let (wrong_completion_order, completion_states) = shortest_counterexample(
        comp_start,
        |state| state.successors(),
        |state| {
            state.returned
                && state.late_published
                && state.cell_occupied
                && state.poll_asleep
                && !state.wake_token
                && !state.timeout_drain_enabled
        },
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_completion_order.len(),
        5,
        "shortest publish-after-wake schedule"
    );
    let (wrong_completion_clear, completion_clear_states) = shortest_counterexample(
        comp_start,
        |state| state.successors(),
        |state| {
            state.returned
                && state.cell_occupied
                && !state.hint
                && state.poll_returned
                && state.poll_asleep
                && !state.wake_token
                && !state.timeout_drain_enabled
        },
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_completion_clear.len(),
        6,
        "shortest clear-hint-without-read schedule"
    );

    let (wrong_identity, identity_states) = shortest_counterexample(
        IdentityMutant {
            old_result_pending: false,
            old_retired: false,
            current_binding: 1,
            result_binding: 0,
            installed_binding: 0,
        },
        |state| state.successors(),
        |state| state.current_binding == 2 && state.installed_binding == 1,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_identity.len(),
        4,
        "shortest omitted binding/transfer_seq schedule"
    );

    let (wrong_capacity, capacity_states) = shortest_counterexample(
        CapacityMutant {
            lane_free: false,
            checked_busy_outside_k2: false,
            marker_registered: false,
            lost_wake: false,
            sleeping: false,
        },
        |state| state.successors(),
        |state| state.lane_free && state.sleeping && state.lost_wake,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_capacity.len(),
        4,
        "shortest capacity lost-wake schedule"
    );

    let deadline_start = DeadlineMutant {
        now: 0,
        worker_running: true,
        owner_count: 1,
        lane_credit: true,
        replacement_admitted: false,
        queue_credit: true,
        tcp_waiting: true,
        reduction_attempted: false,
    };
    let (wrong_deadline_queue, deadline_queue_states) = shortest_counterexample(
        deadline_start,
        |state| state.successors(),
        |state| {
            state.reduction_attempted
                && state.now == 30
                && state.worker_running
                && state.queue_credit
                && state.tcp_waiting
        },
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_deadline_queue.len(),
        2,
        "shortest deadline queue-release failure"
    );
    let (wrong_deadline_physical, deadline_physical_states) = shortest_counterexample(
        deadline_start,
        |state| state.successors(),
        |state| state.replacement_admitted && state.owner_count > 1,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_deadline_physical.len(),
        3,
        "shortest physical-credit release failure"
    );

    let (wrong_predeadline, predeadline_states) = shortest_counterexample(
        PreDeadlineMutant {
            now: 0,
            revoked: false,
            receipt_published: false,
            receipt_time: 0,
            receipt_consumed: false,
            queue_credit: true,
            worker_running: true,
            tcp_waiting: true,
            reduction_attempted: false,
        },
        |state| state.successors(),
        |state| {
            state.now == 5
                && state.revoked
                && state.receipt_published
                && state.receipt_time == 5
                && state.reduction_attempted
                && state.queue_credit
                && state.worker_running
                && state.tcp_waiting
        },
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        wrong_predeadline.len(),
        3,
        "shortest pre-deadline release failure"
    );

    let (stale_proof, proof_states) = shortest_counterexample(
        ProofMutant {
            receipt_a_published: false,
            queue_a_released: false,
            a2_admitted: false,
            a2_queue_released: false,
            stale_receipt_consumed: false,
            a_revoked: false,
            a_success_returned: false,
            a_accepted: false,
            a2_retiring: false,
            a2_closed: false,
            a_closed: false,
            ack_a: false,
            ack_a2: false,
            c_granted: false,
        },
        |state| state.successors(),
        |state| state.stale_receipt_consumed && state.a2_queue_released,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        stale_proof.len(),
        4,
        "shortest wrong-flight receipt settlement"
    );
    let (post_proof_accept, post_proof_states) = shortest_counterexample(
        ProofMutant {
            receipt_a_published: false,
            queue_a_released: false,
            a2_admitted: false,
            a2_queue_released: false,
            stale_receipt_consumed: false,
            a_revoked: false,
            a_success_returned: false,
            a_accepted: false,
            a2_retiring: false,
            a2_closed: false,
            a_closed: false,
            ack_a: false,
            ack_a2: false,
            c_granted: false,
        },
        |state| state.successors(),
        |state| state.a_revoked && state.receipt_a_published && state.a_accepted,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        post_proof_accept.len(),
        3,
        "shortest post-revocation authority failure"
    );
    let (old_ack, old_ack_states) = shortest_counterexample(
        ProofMutant {
            receipt_a_published: false,
            queue_a_released: false,
            a2_admitted: false,
            a2_queue_released: false,
            stale_receipt_consumed: false,
            a_revoked: false,
            a_success_returned: false,
            a_accepted: false,
            a2_retiring: false,
            a2_closed: false,
            a_closed: false,
            ack_a: false,
            ack_a2: false,
            c_granted: false,
        },
        |state| state.successors(),
        |state| state.c_granted && state.a2_retiring && !state.a2_closed,
        |action| format!("{action:?}"),
        100,
    );
    assert_eq!(
        old_ack.len(),
        7,
        "shortest old-A acknowledgement releases A2"
    );

    println!(
        "G2-I mutant shortest traces: wrong_clear(states={clear_states}) [{}]; wrong_completion_publish(states={completion_states}) [{}]; wrong_completion_hint(states={completion_clear_states}) [{}]; wrong_identity(states={identity_states}) [{}]; wrong_capacity(states={capacity_states}) [{}]; wrong_deadline_queue(states={deadline_queue_states}) [{}]; wrong_deadline_physical(states={deadline_physical_states}) [{}]; wrong_predeadline(states={predeadline_states}) [{}]; stale_proof(states={proof_states}) [{}]; post_proof_accept(states={post_proof_states}) [{}]; old_ack(states={old_ack_states}) [{}]",
        wrong_clear.join(" -> "),
        wrong_completion_order.join(" -> "),
        wrong_completion_clear.join(" -> "),
        wrong_identity.join(" -> "),
        wrong_capacity.join(" -> "),
        wrong_deadline_queue.join(" -> "),
        wrong_deadline_physical.join(" -> "),
        wrong_predeadline.join(" -> "),
        stale_proof.join(" -> "),
        post_proof_accept.join(" -> "),
        old_ack.join(" -> "),
    );
}
