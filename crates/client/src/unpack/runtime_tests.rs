//! Unit tests for runtime staging sweep (injected liveness) and race-free probes.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "runtime-staging-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn touch_dir(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("marker"), b"x").unwrap();
    path
}

struct LiveChild(std::process::Child);

impl LiveChild {
    fn spawn() -> Self {
        #[cfg(unix)]
        let child = std::process::Command::new("sleep")
            .arg("30")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn sleep");
        #[cfg(windows)]
        let child = std::process::Command::new("timeout")
            .args(["/T", "30", "/NOBREAK"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn timeout");
        #[cfg(not(any(unix, windows)))]
        compile_error!("LiveChild requires unix or windows");
        Self(child)
    }

    fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for LiveChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn sweep_runtime_staging_removes_dead_foreign_only() {
    let tmp = Temp::new();
    let root = &tmp.0;
    let self_pid = 42u32;
    let dead_pid = 100u32;
    let alive_pid = 200u32;

    let dead = touch_dir(root, &format!(".runtime-{dead_pid}-0"));
    let self_dir = touch_dir(root, &format!(".runtime-{self_pid}-999"));
    let alive = touch_dir(root, &format!(".runtime-{alive_pid}-0"));
    let other = touch_dir(root, "not-runtime-staging");
    let bad_name = touch_dir(root, ".runtime-abc-1");
    let pid_zero = touch_dir(root, ".runtime-0-0");
    // Larger than i32::MAX: not a Unix pid_t, so never parsed as staging there.
    // On Windows (DWORD pids) it is a valid owner and the injected probe decides.
    let high_pid = touch_dir(root, &format!(".runtime-{}-1", u32::MAX));

    sweep_runtime_staging(root, self_pid, |pid| pid == alive_pid);

    assert!(!dead.exists(), "dead foreign staging must be removed");
    assert!(self_dir.exists(), "self-pid staging must stay");
    assert!(alive.exists(), "alive foreign staging must stay");
    assert!(other.exists(), "non-matching name must stay");
    assert!(bad_name.exists(), "non-decimal name must stay");
    assert!(
        pid_zero.exists(),
        "pid 0 name must stay (not parsed as staging)"
    );
    #[cfg(unix)]
    assert!(
        high_pid.exists(),
        "a pid beyond pid_t must stay on Unix (not parsed as staging)"
    );
    #[cfg(not(unix))]
    assert!(
        !high_pid.exists(),
        "a dead high DWORD pid is foreign staging on Windows"
    );
}

#[test]
fn process_is_alive_reports_current_pid() {
    assert!(process_is_alive(std::process::id()));
}

#[test]
fn process_is_alive_reports_live_child() {
    let live = LiveChild::spawn();
    assert!(
        process_is_alive(live.id()),
        "live child pid {} should be alive",
        live.id()
    );
}
