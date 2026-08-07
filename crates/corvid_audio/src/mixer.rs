//! The pool of voices and the buffer they are summed into.
//!
//! Everything here runs on the device's own thread, so nothing here allocates,
//! waits, or has a case it cannot answer. The pool is a boxed slice sized once
//! when the mixer is built, [`fill`](Mixer::fill) walks it by iterator so there
//! is no index to be out of range, and the arithmetic is `f32` with every input
//! made finite before it gets in — see [`Voice::start`](crate::Voice::start).
//!
//! It is also the half of this crate that can be tested without a sound card,
//! which is why it is a type of its own rather than a closure inside the
//! backend. `tests/mixer.rs` fills buffers and looks at the samples.

use crate::voice::{Timbre, Voice};

/// One cue, resolved into everything the device thread needs to play it.
///
/// The lookup from a [`SoundId`](corvid_sound::SoundId) to a [`Timbre`], the
/// gain arithmetic and the pitch are all done on the game's thread, so what
/// crosses to the device is a value with nothing left to resolve. That is what
/// keeps the callback from needing a catalogue, a map, or a branch on whether a
/// sound is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// What it sounds like, with the cue's pitch already in the frequency.
    pub timbre: Timbre,
    /// How loud, with the cue's gain and the listener's already multiplied
    /// together.
    pub gain: f32,
}

/// A fixed pool of voices, summed into whatever buffer a device asks for.
///
/// # What it does not do
///
/// It does not spatialize. Every voice goes to every channel at the same
/// amplitude, so a cue thirty metres to the left sounds exactly like one at the
/// listener's feet. The frame carries the positions and this mixer ignores
/// them; `README.md` says what that costs and what would close it.
///
/// It does not resample, so a cue's pitch is a frequency here rather than a
/// playback rate — there is no recording to play faster.
///
/// It does not limit. The sum of the voices is clipped hard at full scale,
/// which is audible as distortion when enough loud cues land on one sample.
/// A limiter is a decision about attack and release that belongs with a game's
/// mix rather than with its first backend.
#[derive(Debug)]
pub struct Mixer {
    /// How many samples a second the device asked for.
    rate: f32,
    /// The pool. Allocated once, never grown, never walked by index.
    voices: Box<[Voice]>,
    /// How many notes have been dropped because every voice was busy and every
    /// one of them was louder. Read by the game's thread, which is the only
    /// place there is anywhere to report it.
    stolen: u64,
}

impl Mixer {
    /// A mixer for a device running at `rate` hertz with `voices` slots.
    ///
    /// This is the one allocation in the whole of the device path, and it
    /// happens before the stream is started.
    #[must_use]
    pub fn new(rate: u32, voices: usize) -> Self {
        Self {
            rate: as_rate(rate),
            voices: vec![Voice::default(); voices.max(1)].into_boxed_slice(),
            stolen: 0,
        }
    }

    /// How many samples a second this mixer was built for.
    #[must_use]
    pub const fn rate(&self) -> f32 {
        self.rate
    }

    /// How many voices are making a sound.
    #[must_use]
    pub fn playing(&self) -> usize {
        self.voices.iter().filter(|voice| voice.playing()).count()
    }

    /// How many notes have been played over the top of a voice that was still
    /// sounding.
    #[must_use]
    pub const fn stolen(&self) -> u64 {
        self.stolen
    }

    /// Starts `note` on a free voice, or on the quietest one if there is none.
    ///
    /// Stealing the quietest rather than the oldest is the choice that is least
    /// audible: the quietest voice is the one closest to being finished, and a
    /// sound cut off at a hundredth of its peak is a sound nobody was listening
    /// to. Stealing the oldest would cut off a long sustained cue in favour of
    /// a click.
    pub fn start(&mut self, note: Note) {
        let mut quietest: Option<(usize, f32)> = None;
        for (index, voice) in self.voices.iter().enumerate() {
            if !voice.playing() {
                quietest = Some((index, -1.0));
                break;
            }
            let loudness = voice.loudness();
            if quietest.is_none_or(|(_, best)| loudness < best) {
                quietest = Some((index, loudness));
            }
        }
        let Some((index, loudness)) = quietest else {
            return;
        };
        if loudness >= 0.0 {
            self.stolen = self.stolen.saturating_add(1);
        }
        if let Some(voice) = self.voices.get_mut(index) {
            voice.start(note.timbre, note.gain, self.rate);
        }
    }

    /// The next sample of the whole mix, every voice summed and clipped.
    ///
    /// One value rather than a buffer, so that a backend converting to whatever
    /// the device wants writes straight into the device's own buffer and needs
    /// no intermediate of its own. That is not a micro-optimisation: an
    /// intermediate has to be sized, and the only honest place to size it is
    /// inside the callback, where allocating is the one thing that is not
    /// allowed.
    pub fn next_sample(&mut self) -> f32 {
        let mut sum = 0.0;
        for voice in &mut *self.voices {
            sum += voice.next_sample();
        }
        sum.clamp(-1.0, 1.0)
    }

    /// Fills `out` with `channels` interleaved copies of the mix.
    ///
    /// Every channel gets the same sample, which is this mixer's whole position
    /// on spatialization: the frame's offsets are carried and not used.
    ///
    /// `out` is written in full, including where there is nothing playing —
    /// a device buffer holds whatever was in it last time, and leaving it alone
    /// repeats the last few milliseconds forever.
    pub fn fill(&mut self, out: &mut [f32], channels: usize) {
        for slot in out.chunks_mut(channels.max(1)) {
            let sample = self.next_sample();
            for sample_out in slot {
                *sample_out = sample;
            }
        }
    }

    /// Stops every voice at once.
    ///
    /// A cut rather than a fade, and it is not for a game to call between
    /// frames: it exists for a backend closing a stream, where the alternative
    /// is the device's own buffer repeating whatever was in it.
    pub fn silence(&mut self) {
        for voice in &mut *self.voices {
            *voice = Voice::default();
        }
    }
}

/// A device's sample rate as something to divide by.
///
/// Zero is what a device that could not say reports, and one hertz is the
/// slowest thing that is still arithmetic.
const fn as_rate(rate: u32) -> f32 {
    if rate == 0 {
        1.0
    } else {
        // A sample rate is at most a few hundred thousand, which every `f32`
        // represents exactly.
        #[allow(
            clippy::cast_precision_loss,
            reason = "a sample rate is well under 2^24, where every integer has an exact f32"
        )]
        let rate = rate as f32;
        rate
    }
}
