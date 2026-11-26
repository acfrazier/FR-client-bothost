//! Sound plane: the 274 `signlink` midi control plane (`Midi`/`NullMidi`/
//! `RustyMidi`) and the procedural SFX path (`JagFX`/`Tone`) that serves
//! `SYNTH_SOUND`. No tinymidipcm, no Spessa.

pub mod jagfx;
pub mod jagfx_377;
pub mod midi;
pub mod output;
pub mod tone;

pub use jagfx::JagFX;
pub use midi::{Midi, NullMidi, PreparedSong, SongPreparer};
pub use output::{Fade, WavePlayback};
pub use tone::Tone;

#[cfg(all(feature = "audio", feature = "test-support"))]
pub use midi::preload_sound_font_async_gated;
#[cfg(feature = "audio")]
pub use midi::{preload_sound_font_async, RustyMidi};
#[cfg(feature = "audio")]
pub use output::{AudioError, AudioOut};
