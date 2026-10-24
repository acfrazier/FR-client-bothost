#![cfg(feature = "render-test-hooks")]

#[path = "common/gpu_serial.rs"]
mod gpu_serial;
#[path = "common/scene_fixture.rs"]
mod scene_fixture;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use client::render::backend::gpu::{
    current_scene_submit_result, set_scene_workspace_hook, SceneSubmit,
};

static HOOKS: LazyLock<()> = LazyLock::new(|| set_scene_workspace_hook(omitted_workspace));

thread_local! {
    static ACTIVE_OBSERVATION: RefCell<Option<Arc<Observation>>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Observation {
    workspace_calls: AtomicUsize,
    callback_result_was_not_run: AtomicBool,
}

struct ObservationScope(Option<Arc<Observation>>);

impl ObservationScope {
    fn enter(observation: Arc<Observation>) -> Self {
        Self(ACTIVE_OBSERVATION.with(|slot| slot.replace(Some(observation))))
    }
}

impl Drop for ObservationScope {
    fn drop(&mut self) {
        ACTIVE_OBSERVATION.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

fn omitted_workspace(_submit: &mut dyn FnMut()) {
    ACTIVE_OBSERVATION.with(|slot| {
        if let Some(observation) = slot.borrow().as_ref() {
            observation.workspace_calls.fetch_add(1, Ordering::Relaxed);
            observation.callback_result_was_not_run.store(
                current_scene_submit_result() == SceneSubmit::NotRun,
                Ordering::Release,
            );
        }
    });
}

#[test]
fn standalone_omitted_workspace_uses_client_fallback() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; standalone fallback needs a GPU scene");
        return;
    }

    let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
        eprintln!("no adapter; standalone fallback test skips");
        return;
    };
    LazyLock::force(&HOOKS);

    let observation = Arc::new(Observation::default());
    let _observation = ObservationScope::enter(Arc::clone(&observation));
    // Do not install an admission hook. The renderer must use its Standalone default.
    let terrain = scene_fixture::terrain_pixel(renderer.game_draw(&mut client));

    assert_ne!(
        terrain, 0,
        "Standalone must submit terrain after the workspace hook omits its closure"
    );
    assert_eq!(observation.workspace_calls.load(Ordering::Relaxed), 1);
    assert!(observation
        .callback_result_was_not_run
        .load(Ordering::Acquire));
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
}
