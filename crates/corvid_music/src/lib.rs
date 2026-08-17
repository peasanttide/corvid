// The README is the front page, and every `rust` block in it is a doctest. Its
// examples reach into both halves of the crate, so it is the page of a build
// that has both: a doctest cannot be conditioned on a library feature, because
// it is compiled as a crate of its own that cannot see one. A build with one
// half gets the paragraph below, which is the honest thing to show rather than
// a page whose examples would not compile.
#![cfg_attr(
    all(feature = "compose", feature = "synth"),
    doc = include_str!("../README.md")
)]
#![cfg_attr(
    not(all(feature = "compose", feature = "synth")),
    doc = "Music that is composed rather than played back: a bar-at-a-time \
           composer, and a MIDI and `SoundFont` synthesizer to sound it. Both \
           halves are behind features and neither is on by default -- turn on \
           `compose`, `synth`, or both. The crate's full documentation is the \
           front page of a build with both, because its examples use both."
)]
#![no_std]

// A bar holds a growable list of voices holding growable lists of notes, and a
// bank holds decoded samples, so this crate needs an allocator. It needs
// nothing else: there is no `std` here under any feature, and no device under
// any of them either.
extern crate alloc;

#[cfg(any(feature = "compose", feature = "synth"))]
mod num;
#[cfg(feature = "compose")]
mod rng;

#[cfg(feature = "compose")]
mod compose;
#[cfg(feature = "sound")]
mod sound;
#[cfg(feature = "synth")]
mod synth;

#[cfg(feature = "compose")]
pub use compose::{
    Bar, Cadence, Chord, Composer, Event, Mode, Motif, MotifId, MotifPool, Note, Ornament,
    Parameters, Quality, Role, Step, Subject, Transform, Voice, contour_similarity,
    parallel_perfects, transform,
};
#[cfg(feature = "sound")]
pub use sound::{Timbre, music_bus, write_bar};
#[cfg(all(feature = "compose", feature = "synth"))]
pub use synth::perform;
#[cfg(feature = "synth")]
pub use synth::{
    Bank, BankError, Generator, GeneratorAmount, GeneratorKind, Instrument, MidiEvent, Preset,
    Sample, SampleKind, Synth, TimedEvent, Waveform, Zone,
};
