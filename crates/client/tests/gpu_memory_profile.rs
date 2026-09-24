//! Isolated process: actual atlas creation/growth/drop accounting.
use client::render::backend::gpu_atlas::GpuAtlas;
#[test]
fn atlas_payload_growth_and_drop_are_counted_once() {
    client::profiling::enable();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("no adapter on this machine; the memory instrumentation test skips");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let before = client::profiling::gpu_bytes();
    let mut atlas = GpuAtlas::new(&device, "profile test", (16, 16), 64, 0);
    assert_eq!(
        client::profiling::gpu_bytes().textures - before.textures,
        16 * 16 * 4
    );
    assert!(atlas.alloc(&device, &queue, 16, 32).is_some());
    assert_eq!(
        client::profiling::gpu_bytes().textures - before.textures,
        16 * 32 * 4
    );
    drop(atlas);
    assert_eq!(client::profiling::gpu_bytes().textures, before.textures);
    assert_eq!(client::profiling::gpu_bytes().buffers, before.buffers);
}
