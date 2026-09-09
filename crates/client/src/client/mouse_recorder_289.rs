//! Primary Class11: sample the published pointer every 50ms, at most 500 entries.
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub(crate) struct MouseSamples {
    pub position: (i32, i32),
    pub samples: VecDeque<(i32, i32)>,
}

impl Default for MouseSamples {
    fn default() -> Self {
        Self {
            position: (-1, -1),
            samples: VecDeque::with_capacity(500),
        }
    }
}

impl MouseSamples {
    pub fn sample(&mut self) {
        if self.samples.len() < 500 {
            self.samples.push_back(self.position);
        }
    }
}

/// Scoped to the client driver; dropping joins, including early returns.
pub(crate) struct Recorder {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(samples: Arc<Mutex<MouseSamples>>) -> Self {
        let (stop, receive) = mpsc::channel();
        let thread = thread::spawn(move || loop {
            samples.lock().unwrap().sample();
            match receive.recv_timeout(Duration::from_millis(50)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => break,
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
