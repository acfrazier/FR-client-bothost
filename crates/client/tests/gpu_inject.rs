// Campaign task 3 (shared-device seam): the panel injects its wgpu
// device/queue into the client's process-wide GPU context before any slot
// renderer exists, so `GpuBackend` renders on the panel's device and the
// panel binds the client's frame texture directly (zero round-trip). This
// test runs in its own process (each `tests/` file is a separate binary),
// so the context it seeds cannot race the self-init selection tests in
// `gpu_backend.rs`/`render_backend.rs`.
use client::render::backend::gpu::{inject_device, GpuBackend};

/// A real headless device on this machine's adapter (`None` = no adapter;
/// the injection tests then skip, mirroring the existing texture tests).
fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("r274 inject test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::default(),
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn injected_device_wins_over_self_init_and_later_injections() {
    let Some((device, queue)) = test_device() else {
        eprintln!("no adapter on this machine; the injection test skips");
        return;
    };
    let Some((other, other_queue)) = test_device() else {
        eprintln!("no adapter on this machine; the injection test skips");
        return;
    };
    // Inject the "panel" device before any renderer exists, then a second
    // (later) injection: the first context wins — a slot renderer must
    // never create its own device after the panel injected.
    inject_device(device.clone(), queue.clone());
    inject_device(other, other_queue);
    // Force a self-init to fail: `try_new` still succeeds, which proves
    // the injected context (not a fresh self-init) satisfies the backend.
    std::env::set_var("R274_TEST_FORCE_NO_GPU", "1");
    let backend = GpuBackend::try_new()
        .expect("the injected device must satisfy the GPU context (the force-no-GPU hook applies to self-init only)");
    std::env::remove_var("R274_TEST_FORCE_NO_GPU");
    assert_eq!(
        backend.device(),
        &device,
        "the backend must render on the first injected (panel) device"
    );
}
