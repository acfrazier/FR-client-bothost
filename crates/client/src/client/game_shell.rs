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
    otim: [u64; 10],
    opos: usize,
    ratio: i32,
    delta: i32,
    count: i32,
}

impl GameShell {
    pub fn new() -> Self {
        GameShell {
            state: 0,
            deltime: 20,
            mindel: 1,
            fps: 0,
            key_held: [0; 128],
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

            self.frame_bookkeeping();
            let delta = self.delta;
            if delta > 0 {
                thread::sleep(Duration::from_millis(delta as u64));
            }

            while self.count < 256 {
                mainloop(self);
                self.count += self.ratio;
            }
            self.count &= 0xff;

            if self.deltime > 0 {
                self.fps = (self.ratio * 1000) / (self.deltime * 256);
            }

            mainredraw(self);
        }

        if self.state == -1 {
            self.shutdown();
        }
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
