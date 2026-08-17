//! Writing a composed bar into a [`corvid_sound::AudioFrame`].
//!
//! The other way to sound a score. [`Synth`](crate::Synth) renders samples and
//! a caller mixes them; this hands the notes to whatever mixer the game already
//! has, as one [`Cue`] per note on a bus of their own. Which is better depends
//! on what the game has: a game with a `SoundFont` bank wants the synthesizer, and
//! a game whose catalogue already holds a recording per instrument wants this,
//! because it gets the platform's own mixer and its own voice budget for free.
//!
//! The score has no position. Every cue written here sits at the listener, so
//! it neither pans nor occludes, which is what "not diegetic" means -- a street
//! singer is a [`Source`](corvid_sound::Source) that a game places itself.

use corvid_fixed::{Factor16, I8F8};
use corvid_sound::{AudioFrame, Bus, BusId, Cue, SoundId};
use corvid_time::Tick;

use crate::compose::{Bar, Role};

/// The recording that stands in for one line's instrument.
///
/// A cue plays a recording at a rate, so a catalogue that answers with one
/// sample per instrument has to say which key that sample was recorded at.
/// Every note is then that recording resampled, which is the same approximation
/// a single-zone `SoundFont` instrument makes and is audible a long way from the
/// root -- so a catalogue with one recording per octave beats one per
/// instrument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timbre {
    /// Which recording.
    pub sound: SoundId,
    /// The MIDI key it was recorded at.
    pub root_key: u8,
}

impl Timbre {
    /// A recording made at `root_key`.
    #[must_use]
    pub const fn new(sound: SoundId, root_key: u8) -> Self {
        Self { sound, root_key }
    }

    /// The playback rate that puts this recording at `key`.
    ///
    /// Equal temperament, and clamped: an [`I8F8`] reaches 128 times the
    /// recorded rate, which is seven octaves, so nothing musical touches the
    /// clamp and a catalogue entry with a nonsense root key lands on it rather
    /// than wrapping.
    #[must_use]
    pub fn rate(self, key: u8) -> I8F8 {
        let semitones = f32::from(key) - f32::from(self.root_key);
        I8F8::from_f64(f64::from(
            libm::exp2f(semitones / 12.0).clamp(0.0078, 127.0),
        ))
    }
}

/// The bus a score plays on: a root under the master, at `gain`.
///
/// A bus of its own is the point. It is what makes "quieter music, unchanged
/// footsteps" one number rather than a pass over every cue, and it is where a
/// game ducks the score under a shout.
///
/// ```
/// use corvid_fixed::Factor16;
/// use corvid_sound::{BusId, AudioFrame};
/// use corvid_music::music_bus;
///
/// const MUSIC: BusId = BusId(3);
///
/// let mut frame = AudioFrame::new();
/// frame.bus(music_bus(MUSIC, Factor16::from_f64(0.7)));
/// assert_eq!(frame.buses.len(), 1);
/// assert_eq!(frame.buses[0].parent, Some(BusId::MASTER));
/// ```
#[must_use]
pub fn music_bus(bus: BusId, gain: Factor16) -> Bus {
    Bus::new(bus).under(BusId::MASTER).with_gain(gain)
}

/// Writes every note of `bar` into `frame` as a cue, and answers how many.
///
/// `timbre` says which recording each [`Role`] plays; a role it answers `None`
/// for is silent, which is how a game with no percussion recording drops the
/// drum without the composer knowing. `downbeat` is the tick the bar starts on
/// and `ticks_per_bar` how long it lasts, so a note's tick is its beat scaled
/// into that span -- the same integer arithmetic a dancer reading the beat does,
/// from the same two numbers.
///
/// Serials come from [`AudioFrame::next_id`], so the cues are numbered after
/// whatever is already in the frame. That makes the numbering reproducible from
/// a serialized frame, and it makes the order this writes in load bearing: it is
/// voice by voice and then note by note, which is the order a bar stores them.
///
/// ```
/// use corvid_fixed::Factor16;
/// use corvid_sound::{AudioFrame, BusId, SoundId};
/// use corvid_time::Tick;
/// use corvid_music::{Composer, Event, Motif, MotifId, Parameters, Role, Step, Timbre, write_bar};
///
/// const MUSIC: BusId = BusId(3);
///
/// let mut composer = Composer::new(11, Parameters::default());
/// composer.motifs_mut().insert(Motif::new(
///     MotifId(1),
///     vec![Event::note(Step::new(0), 1.0), Event::note(Step::new(2), 1.0)],
/// ));
/// let bar = composer.next_bar();
///
/// let mut frame = AudioFrame::new();
/// let written = write_bar(&bar, MUSIC, Tick(60), 30, &mut frame, |role| match role {
///     Role::Lead => Some(Timbre::new(SoundId(1), 72)),
///     Role::Bass => Some(Timbre::new(SoundId(2), 48)),
///     _ => None,
/// });
///
/// assert_eq!(written, frame.cues.len());
/// assert!(frame.cues.iter().all(|cue| cue.bus == MUSIC));
/// ```
pub fn write_bar(
    bar: &Bar,
    bus: BusId,
    downbeat: Tick,
    ticks_per_bar: u32,
    frame: &mut AudioFrame,
    timbre: impl Fn(Role) -> Option<Timbre>,
) -> usize {
    let mut written = 0;
    for voice in &bar.voices {
        let Some(timbre) = timbre(voice.role) else {
            continue;
        };
        for note in &voice.notes {
            let tick = tick_of(bar, downbeat, ticks_per_bar, note.beat);
            let id = frame.next_id(tick);
            frame.cue(
                Cue::new(id, timbre.sound)
                    .on(bus)
                    .with_gain(Factor16::from_f64(f64::from(note.velocity) / 127.0))
                    .with_pitch(timbre.rate(note.key)),
            );
            written += 1;
        }
    }
    written
}

/// The tick `beat` falls on, inside a bar that starts at `downbeat`.
fn tick_of(bar: &Bar, downbeat: Tick, ticks_per_bar: u32, beat: f32) -> Tick {
    if bar.beats <= 0.0 {
        return downbeat;
    }
    let share = (beat / bar.beats).clamp(0.0, 1.0);
    let offset = crate::num::count(share * crate::num::of_u32(ticks_per_bar));
    Tick(
        downbeat
            .0
            .saturating_add(u64::try_from(offset).unwrap_or(0)),
    )
}
