//! The synthesizer: MIDI in, a `SoundFont` bank, and a block of samples out.

mod bank;
mod channel;
mod envelope;
mod gens;
mod hydra;
mod midi;
mod mixer;
#[cfg(feature = "compose")]
mod perform;
mod sf2;
mod voice;

pub use bank::{
    Bank, Generator, GeneratorAmount, GeneratorKind, Instrument, Preset, Sample, SampleKind, Zone,
};
pub use midi::{MidiEvent, TimedEvent};
pub use mixer::Synth;
#[cfg(feature = "compose")]
pub use perform::perform;
pub use sf2::BankError;
pub use voice::Waveform;
