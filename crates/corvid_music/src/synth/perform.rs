//! Turning a composed bar into the messages that sound it.
//!
//! The one place the two halves of this crate meet, and it is deliberately
//! thin: a bar is notes with onsets in beats, a synthesizer wants messages with
//! onsets in frames, and this is that multiplication. Everything else -- which
//! instrument, which bank, how loud -- is a program change and a controller the
//! caller sends itself, because those are a data pack's decisions and not a
//! composer's.

use alloc::vec::Vec;

use crate::compose::{Bar, Role};
use crate::synth::midi::{MidiEvent, TimedEvent};

/// The channel General MIDI reserves for percussion.
const DRUM_CHANNEL: u8 = 9;

/// The channel the `index`th pitched line plays on.
///
/// Nine is stepped over, because on a General MIDI bank it is the percussion
/// channel and a bassoon routed through it comes out as a cowbell.
fn channel_of(index: usize) -> u8 {
    let raw = u8::try_from(index).unwrap_or(15);
    if raw >= DRUM_CHANNEL {
        raw.saturating_add(1).min(15)
    } else {
        raw
    }
}

/// The messages that sound `bar`, starting at frame `start`.
///
/// One channel per line, in the order the lines appear, with percussion on
/// nine. The events come out sorted by frame, so
/// [`Synth::schedule_all`](crate::Synth::schedule_all) can take them as they
/// are.
///
/// A note that runs past the end of the bar keeps its full length: the bar is
/// where a note *starts* being the composer's business, and a note ringing over
/// a barline is how music has always worked. That includes an
/// [elided](Bar::elided) bar, where the notes that had already started are
/// exactly the ones that ring out.
///
/// ```
/// use corvid_music::{Composer, Event, Motif, MotifId, Parameters, Step, Synth, perform};
///
/// let mut composer = Composer::new(9, Parameters::default());
/// composer.motifs_mut().insert(Motif::new(
///     MotifId(1),
///     vec![Event::note(Step::new(0), 1.0), Event::note(Step::new(4), 1.0)],
/// ));
/// let bar = composer.next_bar();
///
/// let mut synth = Synth::new(48_000);
/// synth.schedule_all(perform(&bar, 48_000, 0));
///
/// let frames = (bar.seconds() * 48_000.0) as usize;
/// let mut block = vec![0.0f32; frames * 2];
/// synth.render(&mut block);
/// assert!(block.iter().any(|sample| sample.abs() > 0.001));
/// ```
#[must_use]
pub fn perform(bar: &Bar, sample_rate: u32, start: u64) -> Vec<TimedEvent> {
    let per_beat = if bar.tempo > 0.0 {
        crate::num::of_u32(sample_rate) * 60.0 / bar.tempo
    } else {
        0.0
    };
    let frame = |beat: f32| {
        let offset = u64::try_from(crate::num::frame(f64::from(beat * per_beat))).unwrap_or(0);
        start.saturating_add(offset)
    };

    let mut out: Vec<TimedEvent> = Vec::new();
    let mut pitched = 0usize;
    for voice in &bar.voices {
        let channel = if voice.role == Role::Percussion {
            DRUM_CHANNEL
        } else {
            let channel = channel_of(pitched);
            pitched += 1;
            channel
        };
        for note in &voice.notes {
            out.push(TimedEvent::new(
                frame(note.beat),
                MidiEvent::NoteOn {
                    channel,
                    key: note.key,
                    velocity: note.velocity.clamp(1, 127),
                },
            ));
            out.push(TimedEvent::new(
                frame(note.end()),
                MidiEvent::NoteOff {
                    channel,
                    key: note.key,
                },
            ));
        }
    }
    // A note-off must not be reordered past a note-on that shares its frame, or
    // a repeated key is released the instant it starts.
    out.sort_by_key(|event| {
        (
            event.frame,
            u8::from(matches!(event.event, MidiEvent::NoteOn { .. })),
        )
    });
    out
}
