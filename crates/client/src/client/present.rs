//! Window present behind feature `window`: winit + softbuffer CPU blit of the
//! 789×532 `PixMap`. `pack_rgb` is always-on so the pixel layout compiles
//! headless; only the `Present` window is feature-gated.

/// Pack a PixMap pixel into the softbuffer `u32` layout.
///
/// PixMap pixels are `0x00RRGGBB` in the low 24 bits of an `i32`; softbuffer
/// wants one `u32` per pixel in the same 0x00RRGGBB layout.
pub fn pack_rgb(pix: i32) -> u32 {
    (pix as u32) & 0x00ff_ffff
}

#[cfg(feature = "window")]
pub use window::{Present, PresentError};

#[cfg(feature = "window")]
mod window {
    use std::error::Error;
    use std::fmt;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::time::Duration;

    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::error::EventLoopError;
    use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::keyboard::{Key, NamedKey};
    use winit::platform::pump_events::EventLoopExtPumpEvents;
    use winit::window::{Window, WindowId};

    use crate::client::game_shell::GameShell;
    use crate::client::key_codes::lookup;

    use super::pack_rgb;

    /// The fixed-size applet window. `blit` packs the `PixMap` (via
    /// `pack_rgb`) into the softbuffer surface; `poll` pumps one batch of
    /// winit events into the `GameShell` Java fields.
    pub struct Present {
        event_loop: EventLoop<()>,
        /// `Surface` borrows the display through `&Context`; keep it alive for
        /// the surface's lifetime even though nothing reads it after open.
        #[allow(dead_code)]
        context: softbuffer::Context<Arc<Window>>,
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
        /// `WindowEvent::MouseInput` carries no position; the last
        /// `CursorMoved` coordinates stand in for the Java `mouseDown` x/y.
        cursor: (i32, i32),
        closed: bool,
    }

    /// Why the window could not be created.
    #[derive(Debug)]
    pub enum PresentError {
        EventLoop(EventLoopError),
        Window(winit::error::OsError),
        Softbuffer(softbuffer::SoftBufferError),
        /// A zero `width`/`height` makes no sense for the fixed applet.
        InvalidSize(u32, u32),
    }

    impl Present {
        /// Create the fixed-size window (789×532 applet). No resize-to-fit,
        /// no upscale: the surface is exactly `width`×`height`.
        pub fn open(width: u32, height: u32, title: &str) -> Result<Self, PresentError> {
            let event_loop = EventLoop::new()?;
            #[allow(deprecated)] // pump-based driver creates the window outside run_app
            let window = Arc::new(event_loop.create_window(
                Window::default_attributes()
                    .with_title(title)
                    .with_inner_size(LogicalSize::new(width, height)),
            )?);
            let context = softbuffer::Context::new(window.clone())?;
            let mut surface = softbuffer::Surface::new(&context, window)?;
            let (w, h) = (
                NonZeroU32::new(width).ok_or(PresentError::InvalidSize(width, height))?,
                NonZeroU32::new(height).ok_or(PresentError::InvalidSize(width, height))?,
            );
            surface.resize(w, h)?;
            Ok(Present {
                event_loop,
                context,
                surface,
                cursor: (-1, -1),
                closed: false,
            })
        }

        /// Pack `pixels` (`0x00RRGGBB` `i32`s) into the surface and present
        /// it. Frames are 789×532; anything shorter leaves the tail stale.
        pub fn blit(&mut self, pixels: &[i32], width: u32, height: u32) {
            let mut buffer = match self.surface.buffer_mut() {
                Ok(buffer) => buffer,
                Err(_) => return, // window closed/occluded: nothing to present
            };
            let len = (width as usize).saturating_mul(height as usize);
            for (dst, src) in buffer.iter_mut().zip(pixels.iter().take(len)) {
                *dst = pack_rgb(*src);
            }
            let _ = buffer.present();
        }

        /// Pump one batch of winit events into `shell` via the Task 1
        /// `apply_*` helpers. Returns `false` once the window is closed or
        /// destroyed.
        pub fn poll(&mut self, shell: &mut GameShell) -> bool {
            let mut app = PollApp {
                cursor: &mut self.cursor,
                closed: &mut self.closed,
                shell,
            };
            let _ = self
                .event_loop
                .pump_app_events(Some(Duration::ZERO), &mut app);
            !self.closed
        }
    }

    impl fmt::Display for PresentError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                PresentError::EventLoop(e) => write!(f, "event loop: {e}"),
                PresentError::Window(e) => write!(f, "window: {e}"),
                PresentError::Softbuffer(e) => write!(f, "softbuffer: {e}"),
                PresentError::InvalidSize(w, h) => write!(f, "invalid size {w}x{h}"),
            }
        }
    }

    impl Error for PresentError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            match self {
                PresentError::EventLoop(e) => Some(e),
                PresentError::Window(e) => Some(e),
                PresentError::Softbuffer(e) => Some(e),
                PresentError::InvalidSize(..) => None,
            }
        }
    }

    impl From<EventLoopError> for PresentError {
        fn from(e: EventLoopError) -> Self {
            PresentError::EventLoop(e)
        }
    }

    impl From<winit::error::OsError> for PresentError {
        fn from(e: winit::error::OsError) -> Self {
            PresentError::Window(e)
        }
    }

    impl From<softbuffer::SoftBufferError> for PresentError {
        fn from(e: softbuffer::SoftBufferError) -> Self {
            PresentError::Softbuffer(e)
        }
    }

    /// One poll's event sink: translates winit `WindowEvent`s into the
    /// `GameShell` while borrowing the `Present` state it writes back.
    struct PollApp<'a> {
        cursor: &'a mut (i32, i32),
        closed: &'a mut bool,
        shell: &'a mut GameShell,
    }

    impl ApplicationHandler<()> for PollApp<'_> {
        fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested | WindowEvent::Destroyed => *self.closed = true,
                WindowEvent::CursorMoved { position, .. } => {
                    *self.cursor = (position.x as i32, position.y as i32);
                    self.shell
                        .apply_mouse_move(position.x as i32, position.y as i32);
                }
                WindowEvent::CursorLeft { .. } => {
                    *self.cursor = (-1, -1);
                    self.shell.apply_mouse_move(-1, -1);
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    // Java buttons: 1 left, 2 right (GameShell.ts 152-167).
                    match (state, button) {
                        (ElementState::Pressed, MouseButton::Left) => {
                            self.shell.apply_mouse_down(1, self.cursor.0, self.cursor.1);
                        }
                        (ElementState::Pressed, MouseButton::Right) => {
                            self.shell.apply_mouse_down(2, self.cursor.0, self.cursor.1);
                        }
                        (ElementState::Released, MouseButton::Left | MouseButton::Right) => {
                            self.shell.apply_mouse_up();
                        }
                        _ => {}
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => self.key_event(event),
                _ => {}
            }
        }
    }

    impl PollApp<'_> {
        fn key_event(&mut self, event: KeyEvent) {
            let Some(name) = dom_key(&event.logical_key) else {
                return;
            };
            let Some(java) = lookup(&name) else {
                return;
            };
            self.shell
                .apply_key(event.state == ElementState::Pressed, java.code, java.ch);
        }
    }

    /// DOM `KeyboardEvent.key` string for a winit logical key, limited to the
    /// entries `key_codes::lookup` understands (KeyCodes.ts).
    fn dom_key(key: &Key) -> Option<String> {
        match key {
            Key::Character(c) => Some(c.to_string()),
            Key::Named(named) => {
                let name = match named {
                    NamedKey::ArrowLeft => "ArrowLeft",
                    NamedKey::ArrowRight => "ArrowRight",
                    NamedKey::ArrowUp => "ArrowUp",
                    NamedKey::ArrowDown => "ArrowDown",
                    NamedKey::Enter => "Enter",
                    NamedKey::Backspace => "Backspace",
                    NamedKey::Space => " ",
                    _ => return None,
                };
                Some(name.to_string())
            }
            _ => None,
        }
    }
}
