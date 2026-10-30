//! Explicit byte budgets for render-owned memory.
//!
//! Production owners install a scope only after the render test-hook process
//! fixture has explicitly enabled it. In the absence of a scope, callers keep
//! the legacy allocation path unchanged.

#[cfg(any(test, feature = "render-test-hooks"))]
use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
#[cfg(any(test, feature = "render-test-hooks"))]
use std::marker::PhantomData;
#[cfg(any(test, feature = "render-test-hooks"))]
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A separately bounded render-memory owner.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RenderResource {
    Capture,
    NavVertices,
    NavQuads,
    NavEdges,
    NavInput,
    HeadModels,
    HeadMisc,
    TempModels,
    SharedModels,
    SharedSprites,
    Vertex,
    ContextCpu,
    ContextGpu,
}

impl fmt::Display for RenderResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Capture => "capture",
            Self::NavVertices => "nav vertices",
            Self::NavQuads => "nav quads",
            Self::NavEdges => "nav edges",
            Self::NavInput => "nav input",
            Self::HeadModels => "head models",
            Self::HeadMisc => "head misc",
            Self::TempModels => "temporary models",
            Self::SharedModels => "shared models",
            Self::SharedSprites => "shared sprites",
            Self::Vertex => "vertex storage",
            Self::ContextCpu => "CPU context",
            Self::ContextGpu => "GPU context",
        })
    }
}

/// A failed request for a finite render-memory resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderLimit {
    resource: RenderResource,
    requested: usize,
    maximum: usize,
}

impl RenderLimit {
    pub const fn new(resource: RenderResource, requested: usize, maximum: usize) -> Self {
        Self {
            resource,
            requested,
            maximum,
        }
    }

    pub const fn resource(self) -> RenderResource {
        self.resource
    }

    pub const fn requested(self) -> usize {
        self.requested
    }

    pub const fn maximum(self) -> usize {
        self.maximum
    }
}

impl fmt::Display for RenderLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} budget exceeded: requested {} bytes, maximum {} bytes",
            self.resource, self.requested, self.maximum
        )
    }
}

impl Error for RenderLimit {}

struct ByteBudgetState {
    limit: usize,
    live: AtomicUsize,
    peak: AtomicUsize,
}
#[cfg(any(test, feature = "render-test-hooks"))]
fn arc_allocation_bytes<T>() -> usize {
    // `ArcInner<T>` stores its strong and weak counters before `T`. The
    // budget states are inspected off the frame path, so construct the exact
    // allocation layout from the actual field types rather than an ABI value.
    let (layout, _) = std::alloc::Layout::new::<[AtomicUsize; 2]>()
        .extend(std::alloc::Layout::new::<T>())
        .expect("Arc allocation layout is representable");
    layout.pad_to_align().size()
}

#[cfg(any(test, feature = "render-test-hooks"))]
const RENDER_BUDGET_RESOURCES: [RenderResource; 13] = [
    RenderResource::Capture,
    RenderResource::NavVertices,
    RenderResource::NavQuads,
    RenderResource::NavEdges,
    RenderResource::NavInput,
    RenderResource::HeadModels,
    RenderResource::HeadMisc,
    RenderResource::TempModels,
    RenderResource::SharedModels,
    RenderResource::SharedSprites,
    RenderResource::Vertex,
    RenderResource::ContextCpu,
    RenderResource::ContextGpu,
];

#[cfg(any(test, feature = "render-test-hooks"))]
const PROCESS_RENDER_BUDGET_RESOURCES: [RenderResource; 5] = [
    RenderResource::Capture,
    RenderResource::SharedModels,
    RenderResource::SharedSprites,
    RenderResource::ContextCpu,
    RenderResource::ContextGpu,
];

#[cfg(any(test, feature = "render-test-hooks"))]
const HEAD_RENDER_BUDGET_RESOURCES: [RenderResource; 8] = [
    RenderResource::NavVertices,
    RenderResource::NavQuads,
    RenderResource::NavEdges,
    RenderResource::NavInput,
    RenderResource::HeadModels,
    RenderResource::HeadMisc,
    RenderResource::TempModels,
    RenderResource::Vertex,
];

/// Scalar allocation totals for one render-budget owner.
///
/// `state_allocation_bytes` counts distinct Arc allocation blocks newly owned
/// by this budget relative to `shared_with`. `inline_bytes` is the budget
/// descriptor only; a containing owner that already reports its `size_of`
/// must not add it again. Live render-byte reservations are not consulted.
#[cfg(any(test, feature = "render-test-hooks"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderBudgetScalarReceipt {
    /// Inline `RenderBudget` descriptor bytes, counted by its containing owner.
    pub inline_bytes: usize,
    /// Distinct `ByteBudgetState` Arc allocations owned beyond `shared_with`.
    pub state_allocation_count: usize,
    /// Complete Arc allocation blocks for those distinct states.
    pub state_allocation_bytes: usize,
    /// Budget handles that refer to a state already owned by `shared_with`.
    pub shared_state_handle_count: usize,
}

/// A cheap, cloneable handle to one byte ceiling. Clones reserve against the
/// same atomics.
#[derive(Clone)]
pub struct ByteBudget {
    resource: RenderResource,
    state: Arc<ByteBudgetState>,
}

impl ByteBudget {
    /// Create an explicit finite budget. This is intended for fixtures and
    /// owners configured by the render process; it has no implicit default.
    pub fn new(resource: RenderResource, limit: usize) -> Self {
        Self {
            resource,
            state: Arc::new(ByteBudgetState {
                limit,
                live: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }),
        }
    }

    pub const fn resource(&self) -> RenderResource {
        self.resource
    }

    pub fn maximum(&self) -> usize {
        self.state.limit
    }
    pub(crate) fn same_budget(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub fn live(&self) -> usize {
        self.state.live.load(Ordering::Acquire)
    }

    pub fn peak(&self) -> usize {
        self.state.peak.load(Ordering::Acquire)
    }

    /// Reserve before allocating. The returned owner releases these bytes on
    /// drop, including during unwinding.
    pub fn reserve(&self, bytes: usize) -> Result<BudgetCharge, RenderLimit> {
        let mut live = self.state.live.load(Ordering::Relaxed);
        loop {
            let Some(next) = live.checked_add(bytes) else {
                return Err(RenderLimit::new(
                    self.resource,
                    usize::MAX,
                    self.state.limit,
                ));
            };
            if next > self.state.limit {
                return Err(RenderLimit::new(self.resource, next, self.state.limit));
            }
            match self.state.live.compare_exchange_weak(
                live,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let mut peak = self.state.peak.load(Ordering::Relaxed);
                    while next > peak {
                        match self.state.peak.compare_exchange_weak(
                            peak,
                            next,
                            Ordering::Release,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(observed) => peak = observed,
                        }
                    }
                    return Ok(BudgetCharge {
                        resource: self.resource,
                        state: Arc::clone(&self.state),
                        bytes,
                    });
                }
                Err(observed) => live = observed,
            }
        }
    }
}

/// The non-Clone lifetime owner for one reservation.
pub struct BudgetCharge {
    resource: RenderResource,
    state: Arc<ByteBudgetState>,
    bytes: usize,
}

impl BudgetCharge {
    pub const fn resource(&self) -> RenderResource {
        self.resource
    }

    pub const fn bytes(&self) -> usize {
        self.bytes
    }
    pub(crate) fn belongs_to(&self, budget: &ByteBudget) -> bool {
        Arc::ptr_eq(&self.state, &budget.state)
    }

    /// Grow or shrink a live reservation before changing its allocation.
    pub fn resize(&mut self, bytes: usize) -> Result<(), RenderLimit> {
        if bytes > self.bytes {
            let additional = bytes - self.bytes;
            let budget = ByteBudget {
                resource: self.resource,
                state: Arc::clone(&self.state),
            };
            let mut extra = budget.reserve(additional)?;
            self.bytes = bytes;
            extra.bytes = 0;
        } else if bytes < self.bytes {
            let released = self.bytes - bytes;
            self.state.live.fetch_sub(released, Ordering::AcqRel);
            self.bytes = bytes;
        }
        Ok(())
    }
}

impl Drop for BudgetCharge {
    fn drop(&mut self) {
        self.state.live.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn reserve_replacement(
    budget: &ByteBudget,
    required_len: usize,
    current_capacity: usize,
    element_size: usize,
) -> Result<(usize, BudgetCharge), RenderLimit> {
    let unrepresentable = || RenderLimit::new(budget.resource(), usize::MAX, budget.maximum());
    let required_bytes = required_len
        .checked_mul(element_size)
        .ok_or_else(unrepresentable)?;
    let available_bytes = budget.maximum().saturating_sub(budget.live());
    let affordable_capacity = available_bytes / element_size;
    let geometric_capacity = current_capacity.max(1).saturating_mul(2).max(required_len);
    let candidate_capacity = geometric_capacity
        .min(affordable_capacity)
        .max(required_len);
    let candidate_bytes = candidate_capacity.checked_mul(element_size);
    match candidate_bytes {
        Some(bytes) => match budget.reserve(bytes) {
            Ok(charge) => Ok((candidate_capacity, charge)),
            Err(_) if candidate_capacity > required_len => {
                Ok((required_len, budget.reserve(required_bytes)?))
            }
            Err(error) => Err(error),
        },
        None if candidate_capacity > required_len => {
            Ok((required_len, budget.reserve(required_bytes)?))
        }
        None => Err(unrepresentable()),
    }
}

/// Ensure vector capacity while keeping its charged allocation paired with
/// the vector. Scoped growth admits old and replacement capacity together.
pub(crate) fn reserve_vec<T>(
    values: &mut Vec<T>,
    additional: usize,
    charge: &mut Option<BudgetCharge>,
    budget: Option<&ByteBudget>,
) -> Result<(), RenderLimit> {
    let Some(budget) = budget else {
        values.reserve(additional);
        return Ok(());
    };

    let unrepresentable = || RenderLimit::new(budget.resource(), usize::MAX, budget.maximum());
    let required_len = values
        .len()
        .checked_add(additional)
        .ok_or_else(unrepresentable)?;
    let element_size = std::mem::size_of::<T>();
    let old_bytes = values
        .capacity()
        .checked_mul(element_size)
        .ok_or_else(unrepresentable)?;

    match charge.as_mut() {
        Some(existing) if !existing.belongs_to(budget) => return Err(unrepresentable()),
        Some(existing) => existing.resize(old_bytes)?,
        None => *charge = Some(budget.reserve(old_bytes)?),
    }

    if required_len <= values.capacity() {
        return Ok(());
    }
    if element_size == 0 {
        values
            .try_reserve_exact(additional)
            .map_err(|_| unrepresentable())?;
        return Ok(());
    }

    let (target_capacity, mut new_charge) =
        reserve_replacement(budget, required_len, values.capacity(), element_size)?;

    let mut replacement = Vec::new();
    replacement
        .try_reserve_exact(target_capacity)
        .map_err(|_| unrepresentable())?;
    if replacement.capacity() < target_capacity {
        return Err(unrepresentable());
    }
    let replacement_bytes = replacement
        .capacity()
        .checked_mul(element_size)
        .ok_or_else(unrepresentable)?;
    new_charge.resize(replacement_bytes)?;

    replacement.append(values);
    let old_values = std::mem::replace(values, replacement);
    drop(old_values);
    let old_charge = charge.replace(new_charge);
    drop(old_charge);
    Ok(())
}

/// Ensure deque capacity while keeping its charged allocation paired with
/// the deque. Scoped growth admits old and replacement capacity together.
pub(crate) fn reserve_deque<T>(
    values: &mut VecDeque<T>,
    additional: usize,
    charge: &mut Option<BudgetCharge>,
    budget: Option<&ByteBudget>,
) -> Result<(), RenderLimit> {
    let Some(budget) = budget else {
        values.reserve(additional);
        return Ok(());
    };

    let unrepresentable = || RenderLimit::new(budget.resource(), usize::MAX, budget.maximum());
    let required_len = values
        .len()
        .checked_add(additional)
        .ok_or_else(unrepresentable)?;
    let element_size = std::mem::size_of::<T>();
    let old_bytes = values
        .capacity()
        .checked_mul(element_size)
        .ok_or_else(unrepresentable)?;

    match charge.as_mut() {
        Some(existing) if !existing.belongs_to(budget) => return Err(unrepresentable()),
        Some(existing) => existing.resize(old_bytes)?,
        None => *charge = Some(budget.reserve(old_bytes)?),
    }

    if required_len <= values.capacity() {
        return Ok(());
    }
    if element_size == 0 {
        values
            .try_reserve_exact(additional)
            .map_err(|_| unrepresentable())?;
        return Ok(());
    }

    let (target_capacity, mut new_charge) =
        reserve_replacement(budget, required_len, values.capacity(), element_size)?;
    let mut replacement = VecDeque::new();
    replacement
        .try_reserve_exact(target_capacity)
        .map_err(|_| unrepresentable())?;
    if replacement.capacity() < target_capacity {
        return Err(unrepresentable());
    }
    let replacement_bytes = replacement
        .capacity()
        .checked_mul(element_size)
        .ok_or_else(unrepresentable)?;
    new_charge.resize(replacement_bytes)?;

    replacement.append(values);
    let old_values = std::mem::replace(values, replacement);
    drop(old_values);
    let old_charge = charge.replace(new_charge);
    drop(old_charge);
    Ok(())
}

/// Explicit ceilings for the resources participating in one render fixture.
/// Each ceiling bounds simultaneously live bytes, including old-plus-new
/// replacement. In particular `vertex` is a growth-peak budget, not the
/// backend's separate 128 MiB retained-buffer cap: admitting the largest
/// power-of-two replacement needs 64 + 128 = 192 MiB.
#[cfg(any(test, feature = "render-test-hooks"))]
pub struct RenderBudgetLimits {
    capture: usize,
    nav_vertices: usize,
    nav_quads: usize,
    nav_edges: usize,
    nav_input: usize,
    head_models: usize,
    head_misc: usize,
    temp_models: usize,
    shared_models: usize,
    shared_sprites: usize,
    vertex: usize,
    context_cpu: usize,
    context_gpu: usize,
}

#[cfg(any(test, feature = "render-test-hooks"))]
impl RenderBudgetLimits {
    #[allow(clippy::too_many_arguments)]
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub const fn new(
        capture: usize,
        nav_vertices: usize,
        nav_quads: usize,
        nav_edges: usize,
        nav_input: usize,
        head_models: usize,
        head_misc: usize,
        temp_models: usize,
        shared_models: usize,
        shared_sprites: usize,
        vertex: usize,
        context_cpu: usize,
        context_gpu: usize,
    ) -> Self {
        Self {
            capture,
            nav_vertices,
            nav_quads,
            nav_edges,
            nav_input,
            head_models,
            head_misc,
            temp_models,
            shared_models,
            shared_sprites,
            vertex,
            context_cpu,
            context_gpu,
        }
    }
}

/// A collection of independently enforced render byte budgets.
#[derive(Clone)]
pub struct RenderBudget {
    capture: ByteBudget,
    nav_vertices: ByteBudget,
    nav_quads: ByteBudget,
    nav_edges: ByteBudget,
    nav_input: ByteBudget,
    head_models: ByteBudget,
    head_misc: ByteBudget,
    temp_models: ByteBudget,
    shared_models: ByteBudget,
    shared_sprites: ByteBudget,
    vertex: ByteBudget,
    context_cpu: ByteBudget,
    context_gpu: ByteBudget,
}

impl RenderBudget {
    /// Construct fixture/process budgets only from explicit limits. This does
    /// not activate them; activation requires `scope()` at the opted-in paint.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub fn fixture(limits: RenderBudgetLimits) -> Self {
        Self {
            capture: ByteBudget::new(RenderResource::Capture, limits.capture),
            nav_vertices: ByteBudget::new(RenderResource::NavVertices, limits.nav_vertices),
            nav_quads: ByteBudget::new(RenderResource::NavQuads, limits.nav_quads),
            nav_edges: ByteBudget::new(RenderResource::NavEdges, limits.nav_edges),
            nav_input: ByteBudget::new(RenderResource::NavInput, limits.nav_input),
            head_models: ByteBudget::new(RenderResource::HeadModels, limits.head_models),
            head_misc: ByteBudget::new(RenderResource::HeadMisc, limits.head_misc),
            temp_models: ByteBudget::new(RenderResource::TempModels, limits.temp_models),
            shared_models: ByteBudget::new(RenderResource::SharedModels, limits.shared_models),
            shared_sprites: ByteBudget::new(RenderResource::SharedSprites, limits.shared_sprites),
            vertex: ByteBudget::new(RenderResource::Vertex, limits.vertex),
            context_cpu: ByteBudget::new(RenderResource::ContextCpu, limits.context_cpu),
            context_gpu: ByteBudget::new(RenderResource::ContextGpu, limits.context_gpu),
        }
    }

    /// Create a head-local view of the configured limits. Process-wide
    /// resources retain shared counters; scene-local resources get fresh
    /// counters with the same explicitly supplied ceilings.
    pub fn for_head(&self) -> Self {
        Self {
            capture: self.capture.clone(),
            nav_vertices: ByteBudget::new(RenderResource::NavVertices, self.nav_vertices.maximum()),
            nav_quads: ByteBudget::new(RenderResource::NavQuads, self.nav_quads.maximum()),
            nav_edges: ByteBudget::new(RenderResource::NavEdges, self.nav_edges.maximum()),
            nav_input: ByteBudget::new(RenderResource::NavInput, self.nav_input.maximum()),
            head_models: ByteBudget::new(RenderResource::HeadModels, self.head_models.maximum()),
            head_misc: ByteBudget::new(RenderResource::HeadMisc, self.head_misc.maximum()),
            temp_models: ByteBudget::new(RenderResource::TempModels, self.temp_models.maximum()),
            shared_models: self.shared_models.clone(),
            shared_sprites: self.shared_sprites.clone(),
            vertex: ByteBudget::new(RenderResource::Vertex, self.vertex.maximum()),
            context_cpu: self.context_cpu.clone(),
            context_gpu: self.context_gpu.clone(),
        }
    }

    /// Reconcile this budget's scalar owner allocations without consulting its
    /// logical reservation counters.
    ///
    /// Pass the process template when measuring a head: its five process-wide
    /// handles are reported as shared, while only this head's eight distinct
    /// local Arc allocations contribute new bytes. With `None`, all distinct
    /// states held by this value are reported, including the eight local-state
    /// prototypes retained by the process template.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub fn scalar_allocation_receipt(
        &self,
        shared_with: Option<&Self>,
    ) -> RenderBudgetScalarReceipt {
        let budgets = RENDER_BUDGET_RESOURCES.map(|resource| self.for_resource(resource));
        let shared_budgets = shared_with
            .map(|shared| RENDER_BUDGET_RESOURCES.map(|resource| shared.for_resource(resource)));
        let mut state_allocation_count = 0;
        let mut shared_state_handle_count = 0;
        for (index, budget) in budgets.iter().enumerate() {
            if shared_budgets.is_some_and(|shared| {
                shared
                    .iter()
                    .any(|candidate| Arc::ptr_eq(&budget.state, &candidate.state))
            }) {
                shared_state_handle_count += 1;
            } else if !budgets[..index]
                .iter()
                .any(|previous| Arc::ptr_eq(&budget.state, &previous.state))
            {
                state_allocation_count += 1;
            }
        }
        RenderBudgetScalarReceipt {
            inline_bytes: std::mem::size_of::<Self>(),
            state_allocation_count,
            state_allocation_bytes: state_allocation_count
                * arc_allocation_bytes::<ByteBudgetState>(),
            shared_state_handle_count,
        }
    }

    /// Number of render-budget fields assigned to this head's fresh state.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub const fn head_local_state_count() -> usize {
        HEAD_RENDER_BUDGET_RESOURCES.len()
    }

    /// Number of render-budget fields shared by all configured heads.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub const fn process_shared_state_count() -> usize {
        PROCESS_RENDER_BUDGET_RESOURCES.len()
    }

    /// Number of byte-budget fields in the public render-budget descriptor.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub const fn resource_count() -> usize {
        RENDER_BUDGET_RESOURCES.len()
    }

    /// Arc allocation bytes created for the eight head-local budget states.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub fn head_local_state_allocation_bytes() -> usize {
        HEAD_RENDER_BUDGET_RESOURCES.len() * arc_allocation_bytes::<ByteBudgetState>()
    }

    /// Inline storage for the active render-budget thread-local.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub fn thread_local_storage_bytes() -> usize {
        std::mem::size_of::<RefCell<Option<Self>>>()
    }

    pub fn for_resource(&self, resource: RenderResource) -> &ByteBudget {
        match resource {
            RenderResource::Capture => &self.capture,
            RenderResource::NavVertices => &self.nav_vertices,
            RenderResource::NavQuads => &self.nav_quads,
            RenderResource::NavEdges => &self.nav_edges,
            RenderResource::NavInput => &self.nav_input,
            RenderResource::HeadModels => &self.head_models,
            RenderResource::HeadMisc => &self.head_misc,
            RenderResource::TempModels => &self.temp_models,
            RenderResource::SharedModels => &self.shared_models,
            RenderResource::SharedSprites => &self.shared_sprites,
            RenderResource::Vertex => &self.vertex,
            RenderResource::ContextCpu => &self.context_cpu,
            RenderResource::ContextGpu => &self.context_gpu,
        }
    }

    /// Install this explicitly configured budget for the current thread and
    /// restore any enclosing scope when the returned guard is dropped.
    #[cfg(any(test, feature = "render-test-hooks"))]
    pub fn scope(&self) -> RenderBudgetScope {
        let previous = ACTIVE_RENDER_BUDGET.with(|active| active.replace(Some(self.clone())));
        RenderBudgetScope {
            previous,
            _thread_bound: PhantomData,
        }
    }

    /// Snapshot the current thread's explicit budget handles, if any.
    pub fn current() -> Option<Self> {
        #[cfg(any(test, feature = "render-test-hooks"))]
        {
            ACTIVE_RENDER_BUDGET.with(|active| active.borrow().clone())
        }
        #[cfg(not(any(test, feature = "render-test-hooks")))]
        {
            None
        }
    }

    pub(crate) fn current_resource(resource: RenderResource) -> Option<ByteBudget> {
        #[cfg(any(test, feature = "render-test-hooks"))]
        {
            ACTIVE_RENDER_BUDGET.with(|active| {
                active
                    .borrow()
                    .as_ref()
                    .map(|budget| budget.for_resource(resource).clone())
            })
        }
        #[cfg(not(any(test, feature = "render-test-hooks")))]
        {
            let _ = resource;
            None
        }
    }

    pub(crate) fn is_scoped() -> bool {
        #[cfg(any(test, feature = "render-test-hooks"))]
        {
            ACTIVE_RENDER_BUDGET.with(|active| active.borrow().is_some())
        }
        #[cfg(not(any(test, feature = "render-test-hooks")))]
        {
            false
        }
    }
}

#[cfg(any(test, feature = "render-test-hooks"))]
thread_local! {
    static ACTIVE_RENDER_BUDGET: RefCell<Option<RenderBudget>> = const { RefCell::new(None) };
}

/// Thread-bound RAII render budget scope. It restores nested state on unwind.
#[cfg(any(test, feature = "render-test-hooks"))]
pub struct RenderBudgetScope {
    previous: Option<RenderBudget>,
    _thread_bound: PhantomData<Rc<()>>,
}

#[cfg(any(test, feature = "render-test-hooks"))]
impl Drop for RenderBudgetScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE_RENDER_BUDGET.with(|active| {
            let _ = active.replace(previous);
        });
    }
}

/// Logical byte-owner totals used by the on-demand reconciliation census.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderBytes {
    pub(crate) cpu_surfaces: usize,
    pub(crate) cpu_retained_3d: usize,
    pub(crate) output_pixels: usize,
    pub(crate) capture: usize,
    pub(crate) nav_vertices: usize,
    pub(crate) nav_quads: usize,
    pub(crate) nav_edges: usize,
    pub(crate) nav_input: usize,
    pub(crate) head_models: usize,
    pub(crate) head_misc: usize,
    pub(crate) temp_models: usize,
    pub(crate) shared_models: usize,
    pub(crate) shared_sprites: usize,
    pub(crate) gpu_fixed_textures: usize,
    pub(crate) gpu_fixed_buffers: usize,
    pub(crate) gpu_vertex_live: usize,
    pub(crate) gpu_vertex_retired: usize,
    pub(crate) gpu_nav_target: usize,
    pub(crate) gpu_retained_present: usize,
    pub(crate) gpu_upload_scratch: usize,
    pub(crate) context_cpu: usize,
    pub(crate) context_gpu: usize,
    pub(crate) scalar_state: usize,
}

impl RenderBytes {
    pub const fn cpu_surfaces(self) -> usize {
        self.cpu_surfaces
    }
    pub const fn cpu_retained_3d(self) -> usize {
        self.cpu_retained_3d
    }
    pub const fn output_pixels(self) -> usize {
        self.output_pixels
    }
    pub const fn capture(self) -> usize {
        self.capture
    }
    pub const fn nav_vertices(self) -> usize {
        self.nav_vertices
    }
    pub const fn nav_quads(self) -> usize {
        self.nav_quads
    }
    pub const fn nav_edges(self) -> usize {
        self.nav_edges
    }
    pub const fn nav_input(self) -> usize {
        self.nav_input
    }
    pub const fn head_models(self) -> usize {
        self.head_models
    }
    pub const fn head_misc(self) -> usize {
        self.head_misc
    }
    pub const fn temp_models(self) -> usize {
        self.temp_models
    }
    pub const fn shared_models(self) -> usize {
        self.shared_models
    }
    pub const fn shared_sprites(self) -> usize {
        self.shared_sprites
    }
    pub const fn gpu_fixed_textures(self) -> usize {
        self.gpu_fixed_textures
    }
    pub const fn gpu_fixed_buffers(self) -> usize {
        self.gpu_fixed_buffers
    }
    pub const fn gpu_vertex_live(self) -> usize {
        self.gpu_vertex_live
    }
    pub const fn gpu_vertex_retired(self) -> usize {
        self.gpu_vertex_retired
    }
    pub const fn gpu_nav_target(self) -> usize {
        self.gpu_nav_target
    }
    pub const fn gpu_retained_present(self) -> usize {
        self.gpu_retained_present
    }
    pub const fn gpu_upload_scratch(self) -> usize {
        self.gpu_upload_scratch
    }
    pub const fn context_cpu(self) -> usize {
        self.context_cpu
    }
    pub const fn context_gpu(self) -> usize {
        self.context_gpu
    }
    pub const fn scalar_state(self) -> usize {
        self.scalar_state
    }

    /// Add storage whose lifetime is owned by the slot, not the Renderer:
    /// the CPU image, host nav input and render-backed world allocations.
    pub fn with_host_storage(
        mut self,
        output_pixels: usize,
        nav_input: usize,
        world_storage: usize,
    ) -> Self {
        self.output_pixels = output_pixels;
        self.nav_input = nav_input;
        self.head_misc += world_storage;
        self
    }

    pub const fn total(self) -> usize {
        self.cpu_surfaces
            .saturating_add(self.cpu_retained_3d)
            .saturating_add(self.output_pixels)
            .saturating_add(self.capture)
            .saturating_add(self.nav_vertices)
            .saturating_add(self.nav_quads)
            .saturating_add(self.nav_edges)
            .saturating_add(self.nav_input)
            .saturating_add(self.head_models)
            .saturating_add(self.head_misc)
            .saturating_add(self.temp_models)
            .saturating_add(self.shared_models)
            .saturating_add(self.shared_sprites)
            .saturating_add(self.gpu_fixed_textures)
            .saturating_add(self.gpu_fixed_buffers)
            .saturating_add(self.gpu_vertex_live)
            .saturating_add(self.gpu_vertex_retired)
            .saturating_add(self.gpu_nav_target)
            .saturating_add(self.gpu_retained_present)
            .saturating_add(self.gpu_upload_scratch)
            .saturating_add(self.context_cpu)
            .saturating_add(self.context_gpu)
            .saturating_add(self.scalar_state)
    }
}

pub(crate) fn vec_capacity_bytes<T>(values: &Vec<T>) -> usize {
    values.capacity() * std::mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(base: usize) -> RenderBudget {
        RenderBudget::fixture(RenderBudgetLimits::new(
            base,
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7,
            base + 8,
            base + 9,
            base + 10,
            base + 11,
            base + 12,
        ))
    }

    #[test]
    fn head_budgets_share_process_owners_and_reset_scene_owners() {
        let process = fixture(101);
        let first = process.for_head();
        let second = process.for_head();

        let _capture = first
            .for_resource(RenderResource::Capture)
            .reserve(7)
            .unwrap();
        assert_eq!(second.for_resource(RenderResource::Capture).live(), 7);

        let _vertices = first
            .for_resource(RenderResource::NavVertices)
            .reserve(11)
            .unwrap();
        assert_eq!(second.for_resource(RenderResource::NavVertices).live(), 0);
        assert_eq!(
            second.for_resource(RenderResource::NavVertices).maximum(),
            102
        );
    }

    #[test]
    fn scalar_budget_layout_and_owner_counts_are_receipted() {
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(std::mem::size_of::<RenderResource>(), 1);
            assert_eq!(
                std::mem::size_of::<ByteBudgetState>(),
                3 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<ByteBudget>(),
                2 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<BudgetCharge>(),
                3 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<RenderLimit>(),
                3 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<Option<BudgetCharge>>(),
                std::mem::size_of::<BudgetCharge>()
            );
            assert_eq!(
                std::mem::size_of::<Option<RenderLimit>>(),
                std::mem::size_of::<RenderLimit>()
            );
            assert_eq!(
                std::mem::size_of::<RenderBytes>(),
                23 * std::mem::size_of::<usize>()
            );
            assert_eq!(
                std::mem::size_of::<RenderBudget>(),
                13 * std::mem::size_of::<ByteBudget>()
            );
        }

        assert_eq!(RenderBudget::resource_count(), 13);
        assert_eq!(RenderBudget::head_local_state_count(), 8);
        assert_eq!(RenderBudget::process_shared_state_count(), 5);

        let process = fixture(101);
        let head = process.for_head();
        for resource in PROCESS_RENDER_BUDGET_RESOURCES {
            assert!(process
                .for_resource(resource)
                .same_budget(head.for_resource(resource)));
        }
        for resource in HEAD_RENDER_BUDGET_RESOURCES {
            assert!(!process
                .for_resource(resource)
                .same_budget(head.for_resource(resource)));
        }

        let process_receipt = process.scalar_allocation_receipt(None);
        let head_receipt = head.scalar_allocation_receipt(Some(&process));
        assert_eq!(
            process_receipt.inline_bytes,
            std::mem::size_of::<RenderBudget>()
        );
        assert_eq!(process_receipt.state_allocation_count, 13);
        assert_eq!(
            process_receipt.state_allocation_bytes,
            13 * arc_allocation_bytes::<ByteBudgetState>()
        );
        assert_eq!(process_receipt.shared_state_handle_count, 0);
        assert_eq!(
            head_receipt.inline_bytes,
            std::mem::size_of::<RenderBudget>()
        );
        assert_eq!(head_receipt.state_allocation_count, 8);
        assert_eq!(
            head_receipt.state_allocation_bytes,
            8 * arc_allocation_bytes::<ByteBudgetState>()
        );
        assert_eq!(head_receipt.shared_state_handle_count, 5);
        assert_eq!(
            RenderBudget::head_local_state_allocation_bytes(),
            8 * arc_allocation_bytes::<ByteBudgetState>()
        );
        assert_eq!(
            RenderBudget::thread_local_storage_bytes(),
            std::mem::size_of::<RefCell<Option<RenderBudget>>>()
        );

        println!(
            "T4.4 RenderBudget scalar receipt: resource={} byte_budget_state={} byte_budget={} budget_charge={} render_limit={} option_budget_charge={} option_render_limit={} render_bytes={} render_budget={} process_states={} process_state_bytes={} head_local_states={} head_local_state_bytes={} shared_state_handles={}",
            std::mem::size_of::<RenderResource>(),
            std::mem::size_of::<ByteBudgetState>(),
            std::mem::size_of::<ByteBudget>(),
            std::mem::size_of::<BudgetCharge>(),
            std::mem::size_of::<RenderLimit>(),
            std::mem::size_of::<Option<BudgetCharge>>(),
            std::mem::size_of::<Option<RenderLimit>>(),
            std::mem::size_of::<RenderBytes>(),
            std::mem::size_of::<RenderBudget>(),
            process_receipt.state_allocation_count,
            process_receipt.state_allocation_bytes,
            head_receipt.state_allocation_count,
            head_receipt.state_allocation_bytes,
            head_receipt.shared_state_handle_count,
        );
    }

    #[test]
    fn nested_scope_restores_the_previous_budget() {
        let outer = fixture(101);
        let inner = fixture(201);
        {
            let _outer_scope = outer.scope();
            assert_eq!(
                RenderBudget::current()
                    .unwrap()
                    .for_resource(RenderResource::TempModels)
                    .maximum(),
                108
            );
            {
                let _inner_scope = inner.scope();
                assert_eq!(
                    RenderBudget::current()
                        .unwrap()
                        .for_resource(RenderResource::TempModels)
                        .maximum(),
                    208
                );
            }
            assert_eq!(
                RenderBudget::current()
                    .unwrap()
                    .for_resource(RenderResource::ContextGpu)
                    .maximum(),
                113
            );
        }
        assert!(RenderBudget::current().is_none());
    }

    #[test]
    fn failed_growth_preserves_live_charge_and_drop_releases_it() {
        let budget = ByteBudget::new(RenderResource::TempModels, 8);
        let mut charge = budget.reserve(6).unwrap();
        let error = charge.resize(9).unwrap_err();
        assert_eq!(error, RenderLimit::new(RenderResource::TempModels, 9, 8));
        assert_eq!(budget.live(), 6);
        assert_eq!(budget.peak(), 6);

        charge.resize(3).unwrap();
        assert_eq!(budget.live(), 3);
        drop(charge);
        assert_eq!(budget.live(), 0);
    }

    #[test]
    fn deque_growth_charges_old_and_new_capacity_before_replacement() {
        let mut values = VecDeque::with_capacity(2);
        values.extend([11u32, 22]);
        let old_capacity = values.capacity();
        let old_bytes = old_capacity * std::mem::size_of::<u32>();
        let additional = old_capacity + 1 - values.len();
        let required_bytes = (values.len() + additional) * std::mem::size_of::<u32>();

        let failing_budget =
            ByteBudget::new(RenderResource::NavInput, old_bytes + required_bytes - 1);
        let mut failing_charge = Some(failing_budget.reserve(old_bytes).unwrap());
        let error = reserve_deque(
            &mut values,
            additional,
            &mut failing_charge,
            Some(&failing_budget),
        )
        .unwrap_err();
        assert_eq!(error.resource(), RenderResource::NavInput);
        assert_eq!(values.capacity(), old_capacity);
        assert!(values.iter().copied().eq([11, 22]));
        assert_eq!(failing_budget.live(), old_bytes);
        drop(failing_charge);
        assert_eq!(failing_budget.live(), 0);

        let doubled_capacity = old_capacity * 2;
        let growing_budget = ByteBudget::new(
            RenderResource::NavInput,
            old_bytes + doubled_capacity * std::mem::size_of::<u32>(),
        );
        let mut growing_charge = Some(growing_budget.reserve(old_bytes).unwrap());
        reserve_deque(
            &mut values,
            additional,
            &mut growing_charge,
            Some(&growing_budget),
        )
        .unwrap();
        assert!(values.iter().copied().eq([11, 22]));
        assert!(values.capacity() >= values.len() + additional);
        assert_eq!(
            growing_charge.as_ref().unwrap().bytes(),
            values.capacity() * std::mem::size_of::<u32>()
        );
        assert_eq!(
            growing_budget.live(),
            values.capacity() * std::mem::size_of::<u32>()
        );
    }
}
