// Task 6 (process-wide GPU pipelines): the scene/chrome shader modules
// and the opaque/translucent/chrome pipelines live on the shared
// `GpuContext`, so a second `GpuBackend::try_new` must not
// `create_shader_module` again — two heads pay one shader build. Skips on
// machines without an adapter (the first `try_new` Err). Runs in its own
// process (each `tests/` file is a separate binary), so the process-wide
// GPU context and the shader counter cannot race `gpu_backend.rs`.
use client::render::backend::GpuBackend;

#[test]
fn two_backends_share_one_shader_module_build() {
    let Ok(a) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the shader-sharing test skips");
        return;
    };
    let Ok(b) = GpuBackend::try_new() else {
        panic!("the second try_new must reuse the first process-wide context");
    };
    assert_eq!(
        GpuBackend::tried(),
        2,
        "each backend still asks for the (shared) device once"
    );
    assert_eq!(
        GpuBackend::shader_modules_created(),
        1,
        "a second GpuBackend must not rebuild the shader modules / pipelines"
    );
    assert!(
        std::ptr::eq(a.device(), b.device()),
        "both backends render on the one process-wide device"
    );
}
