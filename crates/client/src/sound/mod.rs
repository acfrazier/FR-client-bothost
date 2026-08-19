//! Sound plane: the 274 `signlink` midi control plane (`Midi`/`NullMidi`/
//! `RustyMidi`) and the procedural SFX path (`JagFX`/`Tone`) that serves
//! `SYNTH_SOUND`. No tinymidipcm, no Spessa.

pub mod jagfx;
pub mod midi;
pub mod tone;

pub use jagfx::JagFX;
pub use midi::{Midi, NullMidi};
pub use tone::Tone;

#[cfg(feature = "audio")]
pub use midi::RustyMidi;
