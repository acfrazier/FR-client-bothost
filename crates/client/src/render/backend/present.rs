//! Per-head retained-present ownership and nonblocking consumer admission.

use super::capture::CaptureGeometry;
use super::readback::ReadbackError;
use std::num::{NonZeroU64, NonZeroUsize};
#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};
#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
use std::time::{Duration, Instant};

#[cfg(feature = "render-diagnostics")]
#[path = "present_diagnostics.rs"]
pub(super) mod diagnostics;

static NEXT_PRESENT_TOKEN: AtomicUsize = AtomicUsize::new(1);
static NEXT_CONSUMER_KEY: AtomicUsize = AtomicUsize::new(1);

/// Identity of one successfully submitted full-frame present.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PresentToken(NonZeroU64);

impl PresentToken {
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

fn next_identity(counter: &AtomicUsize) -> Result<NonZeroU64, ReadbackError> {
    let value = counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map_err(|_| ReadbackError::IdentityExhausted)?;
    let value = u64::try_from(value).map_err(|_| ReadbackError::IdentityExhausted)?;
    NonZeroU64::new(value).ok_or(ReadbackError::IdentityExhausted)
}

fn next_present_token() -> Result<PresentToken, ReadbackError> {
    next_identity(&NEXT_PRESENT_TOKEN).map(PresentToken)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct ConsumerKey(NonZeroU64);

/// The policy applied to a retained game-frame readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadbackKind {
    AnyFrame,
    ValidScene,
}

/// Process-wide bounded count of registered or draining retained textures.
pub struct RetainedPresentBudget {
    live_resources: AtomicUsize,
    cap: OnceLock<NonZeroUsize>,
}

impl RetainedPresentBudget {
    fn process() -> &'static Self {
        static PROCESS: LazyLock<RetainedPresentBudget> = LazyLock::new(|| RetainedPresentBudget {
            live_resources: AtomicUsize::new(0),
            cap: OnceLock::new(),
        });
        &PROCESS
    }

    fn try_acquire(&'static self) -> Result<RetainedPresentPermit, ReadbackError> {
        let cap = self.cap.get().ok_or(ReadbackError::Limit)?.get();
        let mut live = self.live_resources.load(Ordering::Relaxed);
        loop {
            if live >= cap {
                return Err(ReadbackError::Limit);
            }
            let next = live.checked_add(1).ok_or(ReadbackError::Limit)?;
            match self.live_resources.compare_exchange_weak(
                live,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(RetainedPresentPermit {
                        budget: self,
                        #[cfg(any(
                            feature = "render-diagnostics",
                            feature = "draw-memory-profile"
                        ))]
                        logical_bytes: 0,
                    });
                }
                Err(observed) => live = observed,
            }
        }
    }

    fn is_configured(&self) -> bool {
        self.cap.get().is_some()
    }
}

/// Number of retained textures whose registration or held frame has not
/// released the process permit. This includes resources draining after their
/// producer gate is retired.
pub fn process_retained_present_resource_count() -> usize {
    RetainedPresentBudget::process()
        .live_resources
        .load(Ordering::Acquire)
}

#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
#[derive(Default)]
struct RetainedPresentCounters {
    logical_bytes: AtomicUsize,
    allocations: AtomicU64,
    copies: AtomicU64,
    copied_bytes: AtomicU64,
    copy_encode_ns: AtomicU64,
    present_submit_ns: AtomicU64,
}

#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
static PRESENT_COUNTERS: LazyLock<RetainedPresentCounters> =
    LazyLock::new(RetainedPresentCounters::default);

/// Measurement-only process counters for actual retained-present resources.
/// Durations measure CPU command encoding and submission, not GPU execution.
#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedPresentStats {
    live_resources: usize,
    logical_bytes: usize,
    allocations: u64,
    copies: u64,
    copied_bytes: u64,
    copy_encode_time: Duration,
    present_submit_time: Duration,
}

#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
impl RetainedPresentStats {
    pub const fn live_resources(self) -> usize {
        self.live_resources
    }

    pub const fn logical_bytes(self) -> usize {
        self.logical_bytes
    }

    pub const fn allocations(self) -> u64 {
        self.allocations
    }

    pub const fn copies(self) -> u64 {
        self.copies
    }

    pub const fn copied_bytes(self) -> u64 {
        self.copied_bytes
    }

    pub const fn copy_encode_time(self) -> Duration {
        self.copy_encode_time
    }

    pub const fn present_submit_time(self) -> Duration {
        self.present_submit_time
    }
}

/// Read cumulative allocation/copy totals and current live resource totals.
/// Concurrent changes can appear between fields; phase-boundary memory
/// receipts must quiesce their producers before comparing live totals.
#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
pub fn retained_present_stats() -> RetainedPresentStats {
    RetainedPresentStats {
        live_resources: process_retained_present_resource_count(),
        logical_bytes: PRESENT_COUNTERS.logical_bytes.load(Ordering::Acquire),
        allocations: PRESENT_COUNTERS.allocations.load(Ordering::Acquire),
        copies: PRESENT_COUNTERS.copies.load(Ordering::Acquire),
        copied_bytes: PRESENT_COUNTERS.copied_bytes.load(Ordering::Acquire),
        copy_encode_time: Duration::from_nanos(
            PRESENT_COUNTERS.copy_encode_ns.load(Ordering::Acquire),
        ),
        present_submit_time: Duration::from_nanos(
            PRESENT_COUNTERS.present_submit_ns.load(Ordering::Acquire),
        ),
    }
}

#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Configure the sole process retained-present cap before the first consumer
/// registration. A second configuration is rejected, including an identical
/// one, so tests cannot silently replace process state.
pub fn configure_retained_present_budget(cap: NonZeroUsize) -> Result<(), ReadbackError> {
    RetainedPresentBudget::process()
        .cap
        .set(cap)
        .map_err(|_| ReadbackError::Limit)
}

struct RetainedPresentPermit {
    budget: &'static RetainedPresentBudget,
    #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
    logical_bytes: usize,
}

#[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
impl RetainedPresentPermit {
    fn record_allocation(mut self, bytes: usize) -> Self {
        self.logical_bytes = bytes;
        PRESENT_COUNTERS
            .logical_bytes
            .fetch_add(bytes, Ordering::AcqRel);
        PRESENT_COUNTERS.allocations.fetch_add(1, Ordering::Relaxed);
        self
    }
}

impl Drop for RetainedPresentPermit {
    fn drop(&mut self) {
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        PRESENT_COUNTERS
            .logical_bytes
            .fetch_sub(self.logical_bytes, Ordering::AcqRel);
        self.budget.live_resources.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Latest producer state for one backend. The lock protects only the
/// short final copy/submit and consumer-slot transitions.
pub(super) struct PresentGate {
    pub(super) state: Mutex<PresentState>,
    registering: AtomicBool,
    unregister_requested: AtomicBool,
    retire_requested: AtomicBool,
}

pub(super) struct PresentState {
    token: Option<PresentToken>,
    has_valid_scene: bool,
    alive: bool,
    consumer: Option<Arc<RetainedPresent>>,
}

/// One immutable texture allocation per registered GPU head. It is reused
/// only while no ConsumerFrame owns the currently offered bytes.
pub(super) struct RetainedPresent {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) geometry: CaptureGeometry,
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) state: Mutex<RetainedState>,
    release_pending: AtomicBool,
    _storage: crate::profiling::Allocation,
    _permit: RetainedPresentPermit,
    gate: Weak<PresentGate>,
    // Last: observe release only after the actual process permit has dropped.
    #[cfg(feature = "render-diagnostics")]
    diagnostics: diagnostics::Lifetime,
}

impl RetainedPresent {
    fn encode_copy(&self, encoder: &mut wgpu::CommandEncoder, present: &wgpu::Texture) {
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        let started = Instant::now();
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: present,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.geometry.width(),
                height: self.geometry.height(),
                depth_or_array_layers: 1,
            },
        );
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        {
            PRESENT_COUNTERS
                .copy_encode_ns
                .fetch_add(elapsed_ns(started), Ordering::Relaxed);
            PRESENT_COUNTERS.copies.fetch_add(1, Ordering::Relaxed);
            let bytes = self
                .geometry
                .checked_layout()
                .expect("retained present geometry was checked before allocation")
                .0;
            PRESENT_COUNTERS
                .copied_bytes
                .fetch_add(bytes as u64, Ordering::Relaxed);
        }
    }
}

pub(super) struct RetainedState {
    key: ConsumerKey,
    registered: bool,
    token: Option<PresentToken>,
    has_valid_scene: bool,
    held: bool,
    available: bool,
}

/// Non-Clone registration for the one retained texture belonging to a head.
pub struct ConsumerRegistration {
    gate: Arc<PresentGate>,
    retained: Arc<RetainedPresent>,
    key: ConsumerKey,
}

/// Non-Clone accepted frame. Its Drop explicitly releases the held resource;
/// until then the producer must not overwrite the retained texture.
pub struct ConsumerFrame {
    pub(super) retained: Arc<RetainedPresent>,
    key: ConsumerKey,
    token: PresentToken,
    has_valid_scene: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PresentSubmit {
    Legacy,
    Fresh(PresentToken),
    Busy,
    IdentityExhausted,
    Retired,
}

impl PresentGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(PresentState {
                token: None,
                has_valid_scene: false,
                alive: true,
                consumer: None,
            }),
            registering: AtomicBool::new(false),
            unregister_requested: AtomicBool::new(false),
            retire_requested: AtomicBool::new(false),
        })
    }
    pub(super) fn retained_present_bytes(&self) -> usize {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.consumer.as_ref().map_or(0, |retained| {
            retained
                .geometry
                .checked_layout()
                .expect("registered retained geometry was validated before allocation")
                .0
        })
    }

    pub(super) fn retained_present_enabled() -> bool {
        RetainedPresentBudget::process().is_configured()
    }

    /// Run the final display and optional retained copy in one encoder and
    /// one queue submission. Contention submits the already-recorded ordinary
    /// chrome work without publishing a fresh output.
    pub(super) fn submit_present(
        &self,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        source: &wgpu::Texture,
        present: &wgpu::Texture,
        valid_scene: bool,
    ) -> PresentSubmit {
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                queue.submit([encoder.finish()]);
                return PresentSubmit::Busy;
            }
        };
        Self::apply_lifecycle(&mut state, self);
        if !state.alive {
            queue.submit([encoder.finish()]);
            return PresentSubmit::Retired;
        }
        let token = match next_present_token() {
            Ok(token) => token,
            Err(_) => {
                queue.submit([encoder.finish()]);
                return PresentSubmit::IdentityExhausted;
            }
        };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: present,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: source.width(),
                height: source.height(),
                depth_or_array_layers: 1,
            },
        );
        let retained = state.consumer.clone();
        let mut retained_guard = None;
        if let Some(retained) = retained.as_ref() {
            match retained.state.try_lock() {
                Ok(mut retained_state) => {
                    if retained.release_pending.swap(false, Ordering::AcqRel) {
                        retained_state.held = false;
                        retained_state.available = false;
                        #[cfg(feature = "render-diagnostics")]
                        diagnostics::released(&retained_state);
                    }
                    if retained_state.registered && !retained_state.held {
                        retained.encode_copy(&mut encoder, present);
                        retained_state.token = Some(token);
                        retained_state.has_valid_scene = valid_scene;
                        retained_state.available = true;
                    }
                    retained_guard = Some(retained_state);
                }
                Err(std::sync::TryLockError::Poisoned(error)) => {
                    let mut retained_state = error.into_inner();
                    if retained.release_pending.swap(false, Ordering::AcqRel) {
                        retained_state.held = false;
                        retained_state.available = false;
                        #[cfg(feature = "render-diagnostics")]
                        diagnostics::released(&retained_state);
                    }
                    if retained_state.registered && !retained_state.held {
                        retained.encode_copy(&mut encoder, present);
                        retained_state.token = Some(token);
                        retained_state.has_valid_scene = valid_scene;
                        retained_state.available = true;
                    }
                    retained_guard = Some(retained_state);
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
        }
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        let submit_started = Instant::now();
        queue.submit([encoder.finish()]);
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        PRESENT_COUNTERS
            .present_submit_ns
            .fetch_add(elapsed_ns(submit_started), Ordering::Relaxed);
        state.token = Some(token);
        state.has_valid_scene = valid_scene;
        #[cfg(feature = "render-diagnostics")]
        if let Some(held) = retained_guard.as_ref().filter(|held| held.held) {
            diagnostics::emit(
                "presented_while_held",
                held.key.0.get(),
                held.token.map(PresentToken::get),
                Some(token.get()),
                Some(held.held),
                Some(held.registered),
            );
        }
        drop(retained_guard);
        PresentSubmit::Fresh(token)
    }

    fn apply_lifecycle(state: &mut PresentState, gate: &PresentGate) {
        if gate.retire_requested.load(Ordering::Acquire) {
            #[cfg(feature = "render-diagnostics")]
            if state.alive {
                if let Some(retained) = state.consumer.as_ref() {
                    diagnostics::emit(
                        "producer_retired",
                        retained.diagnostics.key,
                        None,
                        None,
                        None,
                        None,
                    );
                }
            }
            state.alive = false;
            gate.unregister_requested.store(true, Ordering::Release);
        }
        Self::apply_unregister(state, &gate.unregister_requested);
    }
    fn apply_unregister(state: &mut PresentState, requested: &AtomicBool) {
        if !requested.swap(false, Ordering::AcqRel) {
            return;
        }
        let Some(retained) = state.consumer.clone() else {
            return;
        };
        let mut retained_state = match retained.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                requested.store(true, Ordering::Release);
                return;
            }
        };
        retained_state.registered = false;
        retained_state.available = false;
        #[cfg(feature = "render-diagnostics")]
        diagnostics::emit(
            "unregistered",
            retained_state.key.0.get(),
            retained_state.token.map(PresentToken::get),
            None,
            Some(retained_state.held),
            Some(retained_state.registered),
        );
        if !retained_state.held {
            state.consumer = None;
        }
    }

    pub(super) fn retire(&self) {
        self.retire_requested.store(true, Ordering::Release);
        self.unregister_requested.store(true, Ordering::Release);
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return,
        };
        Self::apply_lifecycle(&mut state, self);
        state.alive = false;
    }
}

impl Drop for PresentGate {
    fn drop(&mut self) {
        // A gate can outlive its backend through a display handle or
        // registration; make that old identity fail closed for new offers.
        if let Ok(state) = self.state.get_mut() {
            state.alive = false;
        }
    }
}

impl super::TextureHandle {
    /// Allocate and install the one retained present resource for this head.
    /// Registration does not promise the handle's already-published frame;
    /// the next successful present creates the first offer.
    pub fn try_register_consumer(&self) -> Result<ConsumerRegistration, ReadbackError> {
        let gate = self.gate.as_ref().ok_or(ReadbackError::Ungated)?.clone();
        if !self.fresh {
            return Err(ReadbackError::Busy);
        }
        let geometry = CaptureGeometry::new(self.width, self.height);
        let (raw_bytes, _, _) = geometry.checked_layout()?;
        gate.registering
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ReadbackError::Busy)?;
        let _registering = RegisteringGuard(&gate.registering);
        {
            let mut state = match gate.state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return Err(ReadbackError::Busy),
            };
            PresentGate::apply_lifecycle(&mut state, &gate);
            if !state.alive {
                return Err(ReadbackError::Retired);
            }
            if state.consumer.is_some() {
                return Err(ReadbackError::Busy);
            }
        }
        let permit = RetainedPresentBudget::process().try_acquire()?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 retained consumer present"),
            size: wgpu::Extent3d {
                width: geometry.width(),
                height: geometry.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        let permit = permit.record_allocation(raw_bytes);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let key = ConsumerKey(next_identity(&NEXT_CONSUMER_KEY)?);
        let retained = Arc::new(RetainedPresent {
            device: self.device.clone(),
            queue: self.queue.clone(),
            geometry,
            _storage: crate::profiling::Allocation::new(0, raw_bytes as u64),
            texture,
            view,
            state: Mutex::new(RetainedState {
                key,
                registered: true,
                token: None,
                has_valid_scene: false,
                held: false,
                available: false,
            }),
            release_pending: AtomicBool::new(false),
            _permit: permit,
            gate: Arc::downgrade(&gate),
            #[cfg(feature = "render-diagnostics")]
            diagnostics: diagnostics::Lifetime { key: key.0.get() },
        });
        let mut state = match gate.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(ReadbackError::Busy),
        };
        PresentGate::apply_lifecycle(&mut state, &gate);
        if !state.alive {
            return Err(ReadbackError::Retired);
        }
        if state.consumer.is_some() {
            return Err(ReadbackError::Busy);
        }
        state.consumer = Some(Arc::clone(&retained));
        drop(state);
        drop(_registering);
        #[cfg(feature = "render-diagnostics")]
        diagnostics::emit(
            "registered",
            key.0.get(),
            None,
            None,
            Some(false),
            Some(true),
        );
        Ok(ConsumerRegistration {
            gate,
            retained,
            key,
        })
    }
}

struct RegisteringGuard<'a>(&'a AtomicBool);

impl Drop for RegisteringGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl ConsumerRegistration {
    /// The actual registration identity, for binding observation to a host head.
    #[cfg(feature = "render-diagnostics")]
    pub fn diagnostic_key(&self) -> u64 {
        self.key.0.get()
    }

    /// Take the newest unheld offer, if one exists. A frame already held by
    /// this consumer remains immutable until its explicit release.
    pub fn try_take_frame(&self) -> Result<Option<ConsumerFrame>, ReadbackError> {
        let mut gate_state = match self.gate.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(ReadbackError::Busy),
        };
        PresentGate::apply_lifecycle(&mut gate_state, &self.gate);
        if !gate_state.alive {
            return Err(ReadbackError::Retired);
        }
        let Some(current) = gate_state.consumer.as_ref() else {
            return Err(ReadbackError::Retired);
        };
        if !Arc::ptr_eq(current, &self.retained) {
            return Err(ReadbackError::Retired);
        }
        let mut retained_state = match self.retained.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(ReadbackError::Busy),
        };
        if self.retained.release_pending.swap(false, Ordering::AcqRel) {
            retained_state.held = false;
            retained_state.available = false;
            #[cfg(feature = "render-diagnostics")]
            diagnostics::released(&retained_state);
        }
        if !retained_state.registered || retained_state.key != self.key {
            return Err(ReadbackError::Retired);
        }
        if retained_state.held || !retained_state.available {
            return Ok(None);
        }
        let Some(token) = retained_state.token else {
            return Ok(None);
        };
        retained_state.held = true;
        retained_state.available = false;
        #[cfg(feature = "render-diagnostics")]
        diagnostics::emit(
            "held",
            self.key.0.get(),
            Some(token.get()),
            None,
            Some(retained_state.held),
            Some(retained_state.registered),
        );
        Ok(Some(ConsumerFrame {
            retained: Arc::clone(&self.retained),
            key: self.key,
            token,
            has_valid_scene: retained_state.has_valid_scene,
        }))
    }
}

impl Drop for ConsumerRegistration {
    fn drop(&mut self) {
        #[cfg(feature = "render-diagnostics")]
        diagnostics::emit(
            "unregister_requested",
            self.key.0.get(),
            None,
            None,
            None,
            None,
        );
        let mut gate_state = match self.gate.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                self.gate
                    .unregister_requested
                    .store(true, Ordering::Release);
                return;
            }
        };
        if !gate_state
            .consumer
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.retained))
        {
            return;
        }
        match self.retained.state.try_lock() {
            Ok(mut retained_state) => {
                if retained_state.key == self.key {
                    retained_state.registered = false;
                    retained_state.available = false;
                    #[cfg(feature = "render-diagnostics")]
                    diagnostics::emit(
                        "unregistered",
                        self.key.0.get(),
                        retained_state.token.map(PresentToken::get),
                        None,
                        Some(retained_state.held),
                        Some(retained_state.registered),
                    );
                    if !retained_state.held {
                        gate_state.consumer = None;
                    }
                }
            }
            Err(std::sync::TryLockError::Poisoned(error)) => {
                let mut retained_state = error.into_inner();
                if retained_state.key == self.key {
                    retained_state.registered = false;
                    retained_state.available = false;
                    #[cfg(feature = "render-diagnostics")]
                    diagnostics::emit(
                        "unregistered",
                        self.key.0.get(),
                        retained_state.token.map(PresentToken::get),
                        None,
                        Some(retained_state.held),
                        Some(retained_state.registered),
                    );
                    if !retained_state.held {
                        gate_state.consumer = None;
                    }
                }
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                self.gate
                    .unregister_requested
                    .store(true, Ordering::Release);
            }
        }
    }
}

impl ConsumerFrame {
    pub fn present_token(&self) -> PresentToken {
        self.token
    }

    pub fn has_valid_scene(&self) -> bool {
        self.has_valid_scene
    }

    pub fn display_view(&self) -> &wgpu::TextureView {
        &self.retained.view
    }

    pub(super) fn key_matches(&self, state: &RetainedState) -> bool {
        state.key == self.key && state.token == Some(self.token) && state.held
    }

    pub(super) fn release_gate_resource_if_unregistered(&self) {
        #[cfg(feature = "render-diagnostics")]
        diagnostics::emit(
            "release_requested",
            self.key.0.get(),
            Some(self.token.get()),
            None,
            None,
            None,
        );
        let released = {
            let mut state = match self.retained.state.try_lock() {
                Ok(state) => state,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    self.retained.release_pending.store(true, Ordering::Release);
                    #[cfg(feature = "render-diagnostics")]
                    diagnostics::emit(
                        "release_deferred",
                        self.key.0.get(),
                        Some(self.token.get()),
                        None,
                        None,
                        None,
                    );
                    return;
                }
            };
            let retired = self
                .retained
                .gate
                .upgrade()
                .is_none_or(|gate| gate.retire_requested.load(Ordering::Acquire));
            if state.key == self.key && state.held && state.token == Some(self.token) {
                state.held = false;
                state.available = false;
                #[cfg(feature = "render-diagnostics")]
                diagnostics::emit(
                    "released",
                    self.key.0.get(),
                    Some(self.token.get()),
                    None,
                    Some(state.held),
                    Some(state.registered),
                );
            }
            if retired {
                state.registered = false;
                state.available = false;
            }
            !state.registered && !state.held
        };
        if released {
            self.clear_gate_consumer();
        }
    }

    fn clear_gate_consumer(&self) {
        let Some(gate) = self.retained.gate.upgrade() else {
            return;
        };
        let mut state = match gate.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                gate.unregister_requested.store(true, Ordering::Release);
                return;
            }
        };
        if state
            .consumer
            .as_ref()
            .is_some_and(|retained| Arc::ptr_eq(retained, &self.retained))
        {
            state.consumer = None;
        }
    }
}

impl Drop for ConsumerFrame {
    fn drop(&mut self) {
        self.release_gate_resource_if_unregistered();
    }
}

/// Fixed scalar-owner receipts. These do not inspect budget counters or
/// create GPU resources. Device-native allocations are not Rust descriptors.
#[cfg(any(test, feature = "render-test-hooks"))]
pub mod test_hooks {
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct ScalarReceipt {
        gate_allocation_bytes: usize,
        retained_present_allocation_bytes: usize,
        registration_descriptor_bytes: usize,
        held_frame_descriptor_bytes: usize,
        process_service_bytes: usize,
    }

    impl ScalarReceipt {
        pub const fn gate_allocation_bytes(self) -> usize {
            self.gate_allocation_bytes
        }

        pub const fn retained_present_allocation_bytes(self) -> usize {
            self.retained_present_allocation_bytes
        }

        pub const fn registration_descriptor_bytes(self) -> usize {
            self.registration_descriptor_bytes
        }

        pub const fn held_frame_descriptor_bytes(self) -> usize {
            self.held_frame_descriptor_bytes
        }

        /// A draining retained resource keeps its producer's Arc allocation
        /// through its Weak handle, even after the gate payload is dropped.
        pub const fn draining_gate_allocation_bytes(self) -> usize {
            self.gate_allocation_bytes
        }

        pub const fn process_service_bytes(self) -> usize {
            self.process_service_bytes
        }

        pub const fn s_gate(self) -> usize {
            self.gate_allocation_bytes
        }

        /// Maximum fixed descriptors for one active-or-draining consumer.
        /// The gate term covers retirement while new heads are attached;
        /// an actual owner census must not count an active gate twice.
        pub const fn s_consumer(self) -> usize {
            self.retained_present_allocation_bytes
                + self.registration_descriptor_bytes
                + self.held_frame_descriptor_bytes
                + self.draining_gate_allocation_bytes()
        }
    }

    fn arc_allocation_bytes<T>() -> usize {
        std::alloc::Layout::new::<(AtomicUsize, AtomicUsize)>()
            .extend(std::alloc::Layout::new::<T>())
            .expect("fixed retained-present Arc layout fits usize")
            .0
            .pad_to_align()
            .size()
    }

    pub fn scalar_receipt() -> ScalarReceipt {
        #[cfg(any(feature = "render-diagnostics", feature = "draw-memory-profile"))]
        let diagnostic_bytes = std::mem::size_of::<LazyLock<RetainedPresentCounters>>()
            + super::super::readback::diagnostic_storage_bytes()
            + {
                #[cfg(feature = "render-diagnostics")]
                {
                    diagnostics::static_storage_bytes()
                }
                #[cfg(not(feature = "render-diagnostics"))]
                {
                    0
                }
            };
        #[cfg(not(any(feature = "render-diagnostics", feature = "draw-memory-profile")))]
        let diagnostic_bytes = 0;
        ScalarReceipt {
            gate_allocation_bytes: arc_allocation_bytes::<PresentGate>(),
            retained_present_allocation_bytes: arc_allocation_bytes::<RetainedPresent>(),
            registration_descriptor_bytes: std::mem::size_of::<ConsumerRegistration>(),
            held_frame_descriptor_bytes: std::mem::size_of::<ConsumerFrame>(),
            process_service_bytes: std::mem::size_of::<LazyLock<RetainedPresentBudget>>()
                + 2 * std::mem::size_of::<AtomicUsize>()
                + diagnostic_bytes,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        #[cfg(not(feature = "render-diagnostics"))]
        fn production_retained_owner_keeps_the_frozen_layout() {
            // This is the complete owner layout before lifetime observation.
            #[expect(dead_code, reason = "layout-only pre-diagnostics regression witness")]
            struct ProductionOwner {
                device: wgpu::Device,
                queue: wgpu::Queue,
                geometry: CaptureGeometry,
                texture: wgpu::Texture,
                view: wgpu::TextureView,
                state: Mutex<RetainedState>,
                release_pending: AtomicBool,
                storage: crate::profiling::Allocation,
                permit: RetainedPresentPermit,
                gate: Weak<PresentGate>,
            }
            assert_eq!(
                std::mem::size_of::<RetainedPresent>(),
                std::mem::size_of::<ProductionOwner>(),
            );
            assert_eq!(
                std::mem::align_of::<RetainedPresent>(),
                std::mem::align_of::<ProductionOwner>(),
            );
        }

        #[test]
        fn retained_present_scalar_size_tripwires() {
            let receipt = scalar_receipt();
            #[cfg(target_pointer_width = "64")]
            {
                assert_eq!(receipt.registration_descriptor_bytes(), 24);
                assert_eq!(receipt.held_frame_descriptor_bytes(), 32);
            }
            assert!(receipt.gate_allocation_bytes() > std::mem::size_of::<PresentGate>());
            assert!(
                receipt.retained_present_allocation_bytes()
                    > std::mem::size_of::<RetainedPresent>()
            );
            println!("retained-present scalar receipt: {receipt:?}");
        }
    }
}
