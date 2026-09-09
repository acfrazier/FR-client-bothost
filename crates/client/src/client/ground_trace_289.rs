//! One opt-in observation per Client owner, never a gameplay input source.
use std::io::Write;
use std::time::{Duration, Instant};

const CAP: u32 = 64;

pub(crate) struct GroundTrace {
    id: u64,
    epoch: Option<Instant>,
    attempt: Option<Instant>,
    done: bool,
    seq: u32,
    seen: u32,
    clicks: u32,
    downs: u32,
    first_button: i32,
    pub armed: bool,
    pub rendered: bool,
    pub routing: bool,
    pub pending_write: bool,
    pub input_pending: bool,
    pub input: [i64; 3],
    // Last native cursor, physical inner size, scale * 1e6. None on host/test input.
    pub native: Option<[i64; 7]>,
}

impl GroundTrace {
    pub fn from_env(is_289: bool) -> Option<Self> {
        // Short circuit BEFORE env access; no parsing, state or allocation on R274.
        (is_289
            && std::env::var_os("CLIENT_289_GROUND_TRACE").as_deref()
                == Some(std::ffi::OsStr::new("1")))
        .then(Self::new)
    }

    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            epoch: None,
            attempt: None,
            done: false,
            seq: 0,
            seen: 0,
            clicks: 0,
            downs: 0,
            first_button: 0,
            armed: false,
            rendered: false,
            routing: false,
            pending_write: false,
            input_pending: false,
            input: [0; 3],
            native: None,
        }
    }

    pub fn active(&self) -> bool {
        self.attempt.is_some() && !self.done
    }

    pub fn record_down(&mut self, button: i32, x: i32, y: i32) {
        if !self.done {
            self.input = [button as i64, x as i64, y as i64];
            self.downs = self.downs.saturating_add(1);
        }
    }

    pub fn tick(&mut self, button: i32, x: i32, y: i32) {
        self.tick_at(Instant::now(), button, x, y);
    }

    fn tick_at(&mut self, now: Instant, button: i32, x: i32, y: i32) {
        if self.done {
            return;
        }
        // Ignore title-screen downs when entering ingame with no latched click.
        if self.epoch.is_none() && button == 0 {
            self.downs = 0;
        }
        self.epoch.get_or_insert(now);
        if self.expired(now) {
            return;
        }
        let observed_button = if self.downs > 0 {
            self.input[0] as i32
        } else {
            button
        };
        self.input_pending = observed_button != 0;
        if observed_button == 0 {
            return;
        }
        if self.clicks > 0
            && !(self.clicks == 1 && self.first_button == 2 && observed_button == 1 && !self.armed)
        {
            self.complete("superseded_input");
            return;
        }
        self.attempt.get_or_insert(now);
        if self.clicks == 0 {
            self.first_button = observed_button;
        }
        self.clicks += 1;
        self.event(
            "input",
            &[
                ("button", self.input[0]),
                ("x", self.input[1]),
                ("y", self.input[2]),
                ("click", self.clicks as i64),
                ("downs", self.downs as i64),
            ],
        );
        if let Some(n) = self.native {
            self.event(
                "native",
                &[
                    ("cursor_x", n[0]),
                    ("cursor_y", n[1]),
                    ("inner_w", n[2]),
                    ("inner_h", n[3]),
                    ("scale_million", n[4]),
                    ("logical_w_milli", n[5]),
                    ("logical_h_milli", n[6]),
                ],
            );
        } else {
            self.event("native", &[("available", 0)]);
        }
        self.event(
            "latch",
            &[("button", button as i64), ("x", x as i64), ("y", y as i64)],
        );
        if button == 0 {
            self.complete("no_latched_input");
        } else if self.downs > 1 {
            self.complete("coalesced_input");
        }
        self.downs = 0;
    }

    pub fn event(&mut self, stage: &'static str, fields: &[(&'static str, i64)]) {
        if !self.active() || self.expired(Instant::now()) {
            return;
        }
        if self.seq >= CAP - 1 {
            self.complete("event_cap");
            return;
        }
        self.seen |= match stage {
            "walk_arm" => 1,
            "post_render" => 2,
            "source" => 4,
            "movement" => 8,
            "route" => 16,
            "write" => 32,
            _ => 0,
        };
        self.seq += 1;
        let mut out = std::io::stderr().lock();
        let _ = write!(
            out,
            "ground289 id={} seq={} stage={} ",
            self.id, self.seq, stage
        );
        for (key, value) in fields {
            let _ = write!(out, "{key}={value} ");
        }
        let _ = writeln!(out);
    }

    fn expired(&mut self, now: Instant) -> bool {
        if self
            .epoch
            .is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_secs(120))
        {
            self.complete("lifetime_timeout");
        } else if self
            .attempt
            .is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_secs(10))
        {
            self.complete("attempt_timeout");
        }
        self.done
    }

    pub fn complete(&mut self, reason: &'static str) {
        if self.done {
            return;
        }
        self.done = true;
        self.seq += 1;
        let _ = writeln!(
            std::io::stderr().lock(),
            "ground289 id={} seq={} stage=complete reason={} seen={} ",
            self.id,
            self.seq,
            reason,
            self.seen
        );
    }
}

impl Drop for GroundTrace {
    fn drop(&mut self) {
        if !self.done {
            self.complete("owner_drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_and_completion_are_terminal() {
        let mut t = GroundTrace::new();
        t.tick(1, 260, 138);
        for _ in 0..100 {
            t.event("probe", &[]);
        }
        assert!(t.done);
        assert_eq!(t.seq, CAP);
        t.complete("duplicate");
        t.tick(1, 260, 138);
        t.event("probe", &[]);
        assert_eq!(t.seq, CAP);
    }

    #[test]
    fn late_render_event_cannot_escape_deadline() {
        let mut t = GroundTrace::new();
        t.tick(1, 260, 138);
        t.attempt = Some(Instant::now() - Duration::from_secs(11));
        t.event("post_render", &[("x", 50)]);
        assert!(t.done);
        assert_eq!(t.seen, 0);
    }

    #[test]
    fn missing_input_and_missing_draw_have_bounded_lifetimes() {
        let now = Instant::now();
        let mut idle = GroundTrace::new();
        idle.tick_at(now, 0, 0, 0);
        assert_eq!(idle.seq, 0);
        idle.tick_at(now + Duration::from_secs(120), 0, 0, 0);
        assert!(idle.done);
        assert_eq!(idle.seen, 0);
        let mut armed = GroundTrace::new();
        armed.tick_at(now, 1, 260, 138);
        armed.armed = true;
        armed.event("walk_arm", &[]);
        armed.tick_at(now + Duration::from_secs(10), 0, 0, 0);
        assert!(armed.done);
        assert_eq!(armed.seen, 1);
    }

    #[test]
    fn a_new_click_cannot_relabel_an_armed_attempt() {
        let mut t = GroundTrace::new();
        t.tick(2, 260, 138);
        t.tick(1, 265, 169);
        assert!(t.active());
        t.armed = true;
        t.tick(1, 20, 20);
        assert!(t.done);
        assert_eq!(t.clicks, 2);
        assert!(GroundTrace::from_env(false).is_none());
    }
}
