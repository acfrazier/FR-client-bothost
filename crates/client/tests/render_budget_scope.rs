#![cfg(any(feature = "render-diagnostics", feature = "render-test-hooks"))]
//! Integration coverage enables a render feature and compiles the library without its `cfg(test)` path.

use client::render::budget::{RenderBudget, RenderBudgetLimits, RenderResource};

fn budget(limit: usize) -> RenderBudget {
    RenderBudget::new(RenderBudgetLimits::new(
        limit, limit, limit, limit, limit, limit, limit, limit, limit, limit, limit, limit, limit,
    ))
}

#[test]
fn explicit_budget_scope_is_real_and_thread_local_when_enabled() {
    assert!(RenderBudget::current().is_none());
    let owner = budget(64);
    {
        let _scope = owner.scope();
        let active = RenderBudget::current().expect("configured scope must be active");
        let capture = active.for_resource(RenderResource::Capture).unwrap();
        let charge = capture.reserve(32).unwrap();
        assert_eq!(capture.live(), 32);
        assert_eq!(
            owner.for_resource(RenderResource::Capture).unwrap().live(),
            32
        );
        std::thread::spawn(|| assert!(RenderBudget::current().is_none()))
            .join()
            .unwrap();
        drop(charge);
        assert_eq!(capture.live(), 0);
    }
    assert!(RenderBudget::current().is_none());
    assert_eq!(
        owner.for_resource(RenderResource::Capture).unwrap().peak(),
        32
    );
}

#[test]
fn nested_budget_scope_restores_the_owner_after_unwind() {
    let outer = budget(64);
    let inner = budget(8);
    let _scope = outer.scope();
    let charge = outer
        .for_resource(RenderResource::Capture)
        .unwrap()
        .reserve(32)
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _scope = inner.scope();
        let active = RenderBudget::current().unwrap();
        assert_eq!(
            active
                .for_resource(RenderResource::Capture)
                .unwrap()
                .maximum(),
            8
        );
        let _charge = active
            .for_resource(RenderResource::Capture)
            .unwrap()
            .reserve(8)
            .unwrap();
        panic!("exercise scope restoration");
    }));
    assert!(result.is_err());
    let restored = RenderBudget::current().expect("outer owner must survive unwind");
    let restored = restored.for_resource(RenderResource::Capture).unwrap();
    assert_eq!(restored.maximum(), 64);
    assert_eq!(restored.live(), 32);
    assert_eq!(
        inner.for_resource(RenderResource::Capture).unwrap().live(),
        0
    );
    drop(charge);
    assert_eq!(restored.live(), 0);
}
