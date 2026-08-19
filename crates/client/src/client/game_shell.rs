//! GameShell timing machine, ported from client-ts `src/client/GameShell.ts`
//! `run` (lines 125-227). DOM/canvas listeners are stripped; only the
//! ratio/count catch-up loop remains. `deltime` is 20 ms, not a 600 ms tick.

use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

pub struct GameShell {
    pub state: i32,
    pub deltime: i32,
    pub mindel: i32,
    pub fps: i32,
    /// Key-down flags, Java `GameShell.keyHeld` (`int[128]`); `tryMove` reads
    /// index 5 for the run flag in the walk packet.
    pub key_held: [i32; 128],
    /// Java `GameShell.mouseX`/`mouseY`; -1 while the pointer is off-canvas.
    pub mouse_x: i32,
    pub mouse_y: i32,
    /// Java `GameShell.mouseButton`: 0 none, 1 left, 2 right.
    pub mouse_button: i32,
    /// Click latched each mainloop pass from `next_mouse_click_*`
    /// (GameShell.ts 186-190).
    pub mouse_click_button: i32,
    pub mouse_click_x: i32,
    pub mouse_click_y: i32,
    next_mouse_click_button: i32,
    next_mouse_click_x: i32,
    next_mouse_click_y: i32,
    /// Text/key ring, Java `GameShell.keyQueue` (`number[]`); the write index
    /// wraps with mask `0x7f`.
    pub key_queue: [i32; 128],
    pub key_queue_write: usize,
    otim: [u64; 10],
    opos: usize,
    /// `pub(crate)` so `Client::run` can drive the catch-up loop itself.
    pub(crate) ratio: i32,
    delta: i32,
    pub(crate) count: i32,
}

impl GameShell {
    pub fn new() -> Self {
        GameShell {
            state: 0,
            deltime: 20,
            mindel: 1,
            fps: 0,
            key_held: [0; 128],
            mouse_x: -1,
            mouse_y: -1,
            mouse_button: 0,
            mouse_click_button: 0,
            mouse_click_x: -1,
            mouse_click_y: -1,
            next_mouse_click_button: 0,
            next_mouse_click_x: -1,
            next_mouse_click_y: -1,
            key_queue: [0; 128],
            key_queue_write: 0,
            otim: [Self::now_millis(); 10],
            opos: 0,
            ratio: 256,
            delta: 1,
            count: 0,
        }
    }

    /// Monotonic ms clock, standing in for `performance.now()`.
    fn now_millis() -> u64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    pub fn set_framerate(&mut self, rate: i32) {
        self.deltime = 1000i32.checked_div(rate).unwrap_or(0);
    }

    /// Copy pending `next_mouse_click_*` onto `mouse_click_*` and clear the
    /// button, as `GameShell.run` does at the top of each mainloop pass
    /// (GameShell.ts 186-190).
    pub fn latch_click(&mut self) {
        self.mouse_click_button = self.next_mouse_click_button;
        self.mouse_click_x = self.next_mouse_click_x;
        self.mouse_click_y = self.next_mouse_click_y;
        self.next_mouse_click_button = 0;
    }

    /// Java `mouseDown`: set position/button and latch a click. Java buttons:
    /// 1 left, 2 right.
    pub fn apply_mouse_down(&mut self, button: i32, x: i32, y: i32) {
        self.mouse_x = x;
        self.mouse_y = y;
        self.mouse_button = button;
        self.next_mouse_click_button = button;
        self.next_mouse_click_x = x;
        self.next_mouse_click_y = y;
    }

    /// Java `mouseUp`: release the button.
    pub fn apply_mouse_up(&mut self) {
        self.mouse_button = 0;
    }

    /// Java `mouseMove`/`pointerEnter`: update the position only.
    pub fn apply_mouse_move(&mut self, x: i32, y: i32) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Key down/up: set `key_held[java_code]` for codes in 0..127; on key-down
    /// enqueue `ch` into the `key_queue` ring.
    pub fn apply_key(&mut self, down: bool, java_code: i32, ch: i32) {
        if (0..127).contains(&java_code) {
            self.key_held[java_code as usize] = if down { 1 } else { 0 };
        }
        if down {
            self.key_queue[self.key_queue_write] = ch;
            self.key_queue_write = (self.key_queue_write + 1) & 0x7f;
        }
    }

    /// Overridable mainloop hook; `Client` supplies the real body via `run`.
    pub fn mainloop(&mut self) {}

    /// Overridable mainredraw hook; `Client` supplies the real body via `run`.
    pub fn mainredraw(&mut self) {}

    /// Run one mainloop pass with the given hook.
    pub fn mainloop_count<F: FnMut(&mut Self)>(&mut self, mut hook: F) {
        hook(self);
    }

    /// One mainloop iteration plus ratio bookkeeping from `GameShell.run`.
    /// Test hook: never sleeps or redraws.
    pub fn pump_one<F: FnMut(&mut Self)>(&mut self, hook: F) {
        self.frame_bookkeeping();
        self.mainloop_count(hook);
        self.count += self.ratio;
    }

    /// Production loop: ratio/count catch-up from `GameShell.run` (125-227),
    /// sleeping `delta` ms between frames. `mainloop`/`mainredraw` are the
    /// `Client` overrides of the no-op hooks.
    pub fn run<F, G>(&mut self, mut mainloop: F, mut mainredraw: G)
    where
        F: FnMut(&mut Self),
        G: FnMut(&mut Self),
    {
        while self.state >= 0 {
            if self.state > 0 {
                self.state -= 1;
                if self.state == 0 {
                    self.shutdown();
                    return;
                }
            }

            let delta = self.begin_frame();
            if delta > 0 {
                thread::sleep(Duration::from_millis(delta as u64));
            }

            while self.count < 256 {
                mainloop(self);
                self.count += self.ratio;
            }
            self.count &= 0xff;
            self.end_frame();

            mainredraw(self);
        }

        if self.state == -1 {
            self.shutdown();
        }
    }

    /// Frame bookkeeping plus the sleep delta for the current frame. Drivers
    /// that run the machine themselves (`Client::run`) call this once per
    /// frame before sleeping; `run` uses it internally.
    pub fn begin_frame(&mut self) -> i32 {
        self.frame_bookkeeping();
        self.delta
    }

    /// FPS bookkeeping from `GameShell.run` (after the mainloop pass).
    pub fn end_frame(&mut self) {
        if self.deltime > 0 {
            self.fps = (self.ratio * 1000) / (self.deltime * 256);
        }
    }

    /// Java `shutdown()`: mark the machine stopped (`state = -2`).
    pub fn stop(&mut self) {
        self.shutdown();
    }

    /// Ratio/delta/otim bookkeeping from `GameShell.run` (143-178).
    fn frame_bookkeeping(&mut self) {
        let last_ratio = self.ratio;
        let last_delta = self.delta;
        self.ratio = 300;
        self.delta = 1;

        let now = Self::now_millis();
        let otime = self.otim[self.opos];
        if otime == 0 {
            self.ratio = last_ratio;
            self.delta = last_delta;
        } else if now > otime {
            self.ratio = (self.deltime * 2560) / (now - otime) as i32;
        }

        if self.ratio < 25 {
            self.ratio = 25;
        } else if self.ratio > 256 {
            self.ratio = 256;
            self.delta = self.deltime - (now - otime) as i32 / 10;
        }

        self.otim[self.opos] = now;
        self.opos = (self.opos + 1) % 10;

        if self.delta > 1 {
            for slot in self.otim.iter_mut() {
                if *slot != 0 {
                    *slot += self.delta as u64;
                }
            }
        }

        if self.delta < self.mindel {
            self.delta = self.mindel;
        }
    }

    fn shutdown(&mut self) {
        self.state = -2;
    }
}
