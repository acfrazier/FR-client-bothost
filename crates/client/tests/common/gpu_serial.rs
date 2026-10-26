use std::sync::{Mutex, MutexGuard};

static GPU_SERIAL: Mutex<()> = Mutex::new(());

/// Serialize painted GPU integration cases within one test executable.
pub fn lock() -> MutexGuard<'static, ()> {
    GPU_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Match the client GPU tests' CI skip convention.
pub fn skipped() -> bool {
    std::env::var("SKIP_GPU").as_deref() == Ok("1")
}
