//! One process-wide, bounded asynchronous GPU readback slot.

use super::capture::CaptureGeometry;
use super::present::{ConsumerFrame, PresentToken};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::Instant;

/// Fail-closed outcomes for retained-frame and generic texture readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadbackError {
    Busy,
    Ungated,
    StalePresent,
    InvalidScene,
    Retired,
    Limit,
    Timeout,
    Device,
    IdentityExhausted,
}

/// The one process-wide staging allocation permit. Timed-out mappings keep
/// both their buffer and permit in `quarantine` until their callback resolves.
pub struct ReadbackBudget {
    occupied: AtomicBool,
    quarantine: Mutex<Option<ReadbackStorage>>,
}

impl ReadbackBudget {
    pub fn process() -> &'static Self {
        static PROCESS: LazyLock<ReadbackBudget> = LazyLock::new(|| ReadbackBudget {
            occupied: AtomicBool::new(false),
            quarantine: Mutex::new(None),
        });
        &PROCESS
    }

    /// Admit exactly one checked raw/padded geometry. No staging buffer is
    /// allocated until the caller receives this permit.
    pub fn try_acquire(
        &'static self,
        geometry: CaptureGeometry,
        deadline: Instant,
    ) -> Result<ReadbackPermit, ReadbackError> {
        let (raw_bytes, stage_bytes, _) = geometry.checked_layout()?;
        if deadline <= Instant::now() {
            return Err(ReadbackError::Timeout);
        }
        self.reap_quarantine()?;
        self.occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ReadbackError::Busy)?;
        Ok(ReadbackPermit {
            budget: self,
            geometry,
            stage_bytes,
            raw_bytes,
            deadline,
        })
    }

    fn reap_quarantine(&self) -> Result<(), ReadbackError> {
        let mut quarantine = match self.quarantine.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(ReadbackError::Busy),
        };
        let Some(storage) = quarantine.as_mut() else {
            return Ok(());
        };
        if storage.device.poll(wgpu::PollType::Poll).is_err() {
            return Err(ReadbackError::Busy);
        }
        match storage.completion.try_recv() {
            Ok(Ok(())) => {
                storage.buffer.unmap();
                drop(quarantine.take());
                Ok(())
            }
            Ok(Err(_)) => {
                drop(quarantine.take());
                Ok(())
            }
            Err(mpsc::TryRecvError::Disconnected) | Err(mpsc::TryRecvError::Empty) => {
                Err(ReadbackError::Busy)
            }
        }
    }
}

/// A checked process staging reservation. Dropping an unused permit releases
/// the single slot; a submitted readback moves it into storage until output
/// handoff.
pub struct ReadbackPermit {
    budget: &'static ReadbackBudget,
    geometry: CaptureGeometry,
    stage_bytes: usize,
    raw_bytes: usize,
    deadline: Instant,
}

impl ReadbackPermit {
    pub const fn geometry(&self) -> CaptureGeometry {
        self.geometry
    }

    pub const fn stage_bytes(&self) -> usize {
        self.stage_bytes
    }

    pub const fn raw_bytes(&self) -> usize {
        self.raw_bytes
    }

    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl Drop for ReadbackPermit {
    fn drop(&mut self) {
        self.budget.occupied.store(false, Ordering::Release);
    }
}

/// An in-flight retained-frame copy. Polling never waits; dropping an
/// unresolved mapping transfers its storage into the process quarantine.
pub struct PendingReadback {
    core: PendingCore,
    token: PresentToken,
    has_valid_scene: bool,
}

/// Completed immutable frame bytes. The permit remains charged until the
/// downstream owner releases or transfers the bytes.
pub struct CompletedReadback {
    rgba: Vec<u8>,
    token: PresentToken,
    has_valid_scene: bool,
    geometry: CaptureGeometry,
    permit: ReadbackPermit,
}

impl PendingReadback {
    pub fn poll(&mut self, now: Instant) -> Result<Option<CompletedReadback>, ReadbackError> {
        let Some(completed) = self.core.poll(now)? else {
            return Ok(None);
        };
        Ok(Some(CompletedReadback {
            rgba: completed.rgba,
            token: self.token,
            has_valid_scene: self.has_valid_scene,
            geometry: self.core.geometry,
            permit: completed.permit,
        }))
    }
}

impl CompletedReadback {
    pub fn into_parts(self) -> (Vec<u8>, PresentToken, bool, CaptureGeometry, ReadbackPermit) {
        (
            self.rgba,
            self.token,
            self.has_valid_scene,
            self.geometry,
            self.permit,
        )
    }
}

/// The same process readback machinery for a panel-owned whole-window
/// texture, which has no actor present token.
pub struct PendingWindowReadback {
    core: PendingCore,
}

/// Completed whole-window bytes and their still-charged staging permit.
pub struct CompletedWindowReadback {
    rgba: Vec<u8>,
    geometry: CaptureGeometry,
    permit: ReadbackPermit,
}

impl PendingWindowReadback {
    pub fn poll(&mut self, now: Instant) -> Result<Option<CompletedWindowReadback>, ReadbackError> {
        let Some(completed) = self.core.poll(now)? else {
            return Ok(None);
        };
        Ok(Some(CompletedWindowReadback {
            rgba: completed.rgba,
            geometry: self.core.geometry,
            permit: completed.permit,
        }))
    }
}

impl CompletedWindowReadback {
    pub fn into_parts(self) -> (Vec<u8>, CaptureGeometry, ReadbackPermit) {
        (self.rgba, self.geometry, self.permit)
    }
}

/// Submit one panel-owned texture through the same process permit/quarantine
/// as retained game-frame reads. The copy is ordered on `queue` before this
/// function returns; mapping is left to nonblocking `poll` calls.
pub fn begin_window_readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Texture,
    permit: ReadbackPermit,
) -> Result<PendingWindowReadback, (ReadbackError, ReadbackPermit)> {
    if CaptureGeometry::new(source.width(), source.height()) != permit.geometry {
        return Err((ReadbackError::Limit, permit));
    }
    let (buffer, encoder) = match prepare_copy(device, source, &permit) {
        Ok(prepared) => prepared,
        Err(error) => return Err((error, permit)),
    };
    queue.submit([encoder.finish()]);
    let storage = begin_map(device, buffer, permit);
    Ok(PendingWindowReadback {
        core: PendingCore {
            geometry: storage.permit.geometry,
            storage: Some(storage),
        },
    })
}

struct PendingCore {
    geometry: CaptureGeometry,
    storage: Option<ReadbackStorage>,
}

impl PendingCore {
    fn poll(&mut self, now: Instant) -> Result<Option<CompletedPixels>, ReadbackError> {
        let Some(storage) = self.storage.as_ref() else {
            return Err(ReadbackError::Device);
        };
        if now >= storage.permit.deadline {
            self.quarantine();
            return Err(ReadbackError::Timeout);
        }
        if self
            .storage
            .as_ref()
            .expect("storage checked above")
            .device
            .poll(wgpu::PollType::Poll)
            .is_err()
        {
            self.quarantine();
            return Err(ReadbackError::Device);
        }
        let completion = self
            .storage
            .as_mut()
            .expect("storage checked above")
            .completion
            .try_recv();
        match completion {
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.quarantine();
                Err(ReadbackError::Device)
            }
            Ok(Err(_)) => {
                self.storage.take();
                Err(ReadbackError::Device)
            }
            Ok(Ok(())) => {
                let storage = self.storage.take().expect("readback storage checked above");
                let ReadbackStorage {
                    buffer,
                    completion: _,
                    permit,
                    padded_bytes_per_row,
                    ..
                } = storage;
                let width_bytes = permit.geometry.width() as usize * 4;
                let slice = buffer.slice(..);
                let data = slice.get_mapped_range();
                let mut rgba = Vec::new();
                let copied = if rgba.try_reserve_exact(permit.raw_bytes).is_err() {
                    Err(ReadbackError::Limit)
                } else {
                    let mut result = Ok(());
                    for row in 0..permit.geometry.height() as usize {
                        let Some(start) = row.checked_mul(padded_bytes_per_row as usize) else {
                            result = Err(ReadbackError::Limit);
                            break;
                        };
                        let Some(end) = start.checked_add(width_bytes) else {
                            result = Err(ReadbackError::Limit);
                            break;
                        };
                        let Some(bytes) = data.get(start..end) else {
                            result = Err(ReadbackError::Device);
                            break;
                        };
                        rgba.extend_from_slice(bytes);
                    }
                    result
                };
                drop(data);
                buffer.unmap();
                match copied {
                    Ok(()) => Ok(Some(CompletedPixels { rgba, permit })),
                    Err(error) => {
                        drop(permit);
                        Err(error)
                    }
                }
            }
        }
    }

    fn quarantine(&mut self) {
        let Some(storage) = self.storage.take() else {
            return;
        };
        let budget = storage.permit.budget;
        let mut slot = budget
            .quarantine
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slot.is_none() {
            *slot = Some(storage);
        } else {
            // One occupied permit makes two quarantined mappings impossible.
            // Preserve the unresolved driver's allocation if an invariant is
            // broken rather than dropping a buffer the device may still write.
            std::mem::forget(storage);
            debug_assert!(false, "readback permit admitted two quarantined buffers");
        }
    }
}

impl Drop for PendingCore {
    fn drop(&mut self) {
        self.quarantine();
    }
}

struct CompletedPixels {
    rgba: Vec<u8>,
    permit: ReadbackPermit,
}

struct ReadbackStorage {
    buffer: wgpu::Buffer,
    completion: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    permit: ReadbackPermit,
    padded_bytes_per_row: u32,
    device: wgpu::Device,
}

fn prepare_copy(
    device: &wgpu::Device,
    source: &wgpu::Texture,
    permit: &ReadbackPermit,
) -> Result<(wgpu::Buffer, wgpu::CommandEncoder), ReadbackError> {
    let (_, stage_bytes, padded_bytes_per_row) = permit.geometry.checked_layout()?;
    if stage_bytes != permit.stage_bytes {
        return Err(ReadbackError::Limit);
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("r274 bounded readback staging"),
        size: permit.stage_bytes as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("r274 bounded readback encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(permit.geometry.height()),
            },
        },
        wgpu::Extent3d {
            width: permit.geometry.width(),
            height: permit.geometry.height(),
            depth_or_array_layers: 1,
        },
    );
    Ok((buffer, encoder))
}

fn begin_map(
    device: &wgpu::Device,
    buffer: wgpu::Buffer,
    permit: ReadbackPermit,
) -> ReadbackStorage {
    let (sender, completion) = mpsc::channel();
    #[cfg(all(test, feature = "render-test-hooks"))]
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            if let Err((sender, result)) = test_hooks::defer_one_completion(sender, result) {
                let _ = sender.send(result);
            }
        });
    #[cfg(not(all(test, feature = "render-test-hooks")))]
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

    let (_, _, padded_bytes_per_row) = permit
        .geometry
        .checked_layout()
        .expect("permit geometry was validated before allocation");
    ReadbackStorage {
        buffer,
        completion,
        permit,
        padded_bytes_per_row,
        device: device.clone(),
    }
}

impl ConsumerFrame {
    /// Copy this exact held frame into the process staging slot. Any refusal
    /// returns both owners unchanged so the caller may retry or cancel.
    pub fn try_begin_readback(
        self,
        kind: super::present::ReadbackKind,
        permit: ReadbackPermit,
    ) -> Result<PendingReadback, (ReadbackError, Self, ReadbackPermit)> {
        if self.retained.geometry != permit.geometry {
            return Err((ReadbackError::Limit, self, permit));
        }
        if kind == super::present::ReadbackKind::ValidScene && !self.has_valid_scene() {
            return Err((ReadbackError::InvalidScene, self, permit));
        }
        let (buffer, encoder) =
            match prepare_copy(&self.retained.device, &self.retained.texture, &permit) {
                Ok(prepared) => prepared,
                Err(error) => return Err((error, self, permit)),
            };
        let state = match self.retained.state.try_lock() {
            Ok(state) => Some(state),
            Err(std::sync::TryLockError::Poisoned(error)) => Some(error.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        };
        if state.is_none() {
            drop(state);
            drop(encoder);
            drop(buffer);
            return Err((ReadbackError::Busy, self, permit));
        }
        let state = state.expect("the contended branch returned its owners");
        if !self.key_matches(&state) {
            drop(state);
            drop(encoder);
            drop(buffer);
            return Err((ReadbackError::StalePresent, self, permit));
        }
        self.retained.queue.submit([encoder.finish()]);
        let storage = begin_map(&self.retained.device, buffer, permit);
        drop(state);
        self.release_gate_resource_if_unregistered();
        Ok(PendingReadback {
            core: PendingCore {
                geometry: self.retained.geometry,
                storage: Some(storage),
            },
            token: self.present_token(),
            has_valid_scene: self.has_valid_scene(),
        })
    }
}

/// Deterministically hold observation of one real map callback while leaving
/// the driver callback itself free to return.
#[cfg(all(test, feature = "render-test-hooks"))]
pub(crate) mod test_hooks {
    use super::*;

    type MapResult = Result<(), wgpu::BufferAsyncError>;

    enum HoldState {
        Disarmed,
        Armed,
        Held(mpsc::Sender<MapResult>, MapResult),
    }

    static HOLD: Mutex<HoldState> = Mutex::new(HoldState::Disarmed);

    pub(crate) struct ScopedCompletionHold {
        active: bool,
    }

    pub(crate) fn hold_next_map_completion() -> ScopedCompletionHold {
        let mut state = HOLD.lock().unwrap_or_else(|error| error.into_inner());
        assert!(
            matches!(&*state, HoldState::Disarmed),
            "the one-shot map-completion fixture is already active"
        );
        *state = HoldState::Armed;
        ScopedCompletionHold { active: true }
    }

    impl ScopedCompletionHold {
        pub(crate) fn is_held(&self) -> bool {
            matches!(
                &*HOLD.lock().unwrap_or_else(|error| error.into_inner()),
                HoldState::Held(_, _)
            )
        }

        pub(crate) fn release(&mut self) {
            if !self.active {
                return;
            }
            self.active = false;
            let held = {
                let mut state = HOLD.lock().unwrap_or_else(|error| error.into_inner());
                match std::mem::replace(&mut *state, HoldState::Disarmed) {
                    HoldState::Held(sender, result) => Some((sender, result)),
                    HoldState::Disarmed | HoldState::Armed => None,
                }
            };
            if let Some((sender, result)) = held {
                let _ = sender.send(result);
            }
        }
    }

    impl Drop for ScopedCompletionHold {
        fn drop(&mut self) {
            self.release();
        }
    }

    pub(super) fn defer_one_completion(
        sender: mpsc::Sender<MapResult>,
        result: MapResult,
    ) -> Result<(), (mpsc::Sender<MapResult>, MapResult)> {
        let mut state = HOLD.lock().unwrap_or_else(|error| error.into_inner());
        if matches!(&*state, HoldState::Armed) {
            *state = HoldState::Held(sender, result);
            Ok(())
        } else {
            Err((sender, result))
        }
    }
}
