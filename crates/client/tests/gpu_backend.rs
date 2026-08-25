// Task 7 (wgpu backend) selection: `client-play --window` prefers the wgpu
// backend where an adapter exists, and still produces a frame either way.
// The force-failure fallback is pinned in `render_backend.rs` (a separate
// process, so the process-wide GPU context and the `R274_TEST_FORCE_NO_GPU`
// hook cannot race this test). The mesh-builder test lives in `gpu_mesh.rs`
// for the same reason (it pins the colour-table brightness).
use client::render::backend::{BackendKind, FrameOutput};
use client::render::Renderer;

/// On a machine with an adapter the renderer selects `GpuBackend` and both
/// a title frame and an in-game frame come back as owned frames; on a
/// machine without one it degrades to the CPU backend — the same graceful
/// fallback the force-failure test pins.
#[test]
fn gpu_backend_selected_when_adapter_available() {
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; the GPU selection test skips (CPU fallback active)");
        Renderer::set_prefer_gpu(false);
        return;
    }

    let mut c = client();
    c.set_draw(true);
    let title = r.title_screen_draw(&mut c);
    assert!(matches!(title, FrameOutput::PixMap(_)), "the GPU title frame must present");

    c.ingame = true;
    let game = r.game_draw(&mut c);
    assert!(
        matches!(game, FrameOutput::PixMap(_)),
        "the GPU in-game frame must present (composited scene + CPU chrome)"
    );

    Renderer::set_prefer_gpu(false);
}

fn client() -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}
