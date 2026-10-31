pub mod backend;
pub mod budget;
#[cfg(feature = "render-diagnostics")]
pub mod diagnostics;
pub mod draw;
pub(crate) mod graphic_sprites;
pub mod media;
#[cfg(feature = "draw-memory-case")]
pub mod memory_case;
pub mod nav_debug;
// Frozen pre-GPU Pix2D oracle for parity tests. Never update it alongside
// production nav rendering; intentional behavior changes need independent
// expected-output review.
#[cfg(test)]
#[allow(dead_code)]
pub mod nav_debug_old;
pub mod renderer;
pub mod store;
pub mod world;
pub use draw::{npc_overlay_box, project_area_game};
pub use media::Media;
pub use renderer::Renderer;
pub use world::RenderWorld;
