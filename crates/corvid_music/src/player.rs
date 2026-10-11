//! The synthesizer and the song it is playing, filling buffers on demand.

use rameau_clip::AudioClip;
use rameau_playback::Timestamp;
use rameau_software::Software;
use rameau_synthesizer::Synthesizer;

use crate::{Bank, Error, Song};

/// How many frames are rendered at a time. Events are dispatched a block
/// ahead and placed inside it to the sample by the mixer, so this bounds
/// nothing about timing; it bounds how much one call to the mixer does.
const BLOCK: usize = 512;

/// Seconds a song is held as playing after its last event, so the release
/// of its last chord rings out before the next song may start.
const TAIL: f64 = 2.0;

/// A song being played: which, how far through its events, and when on the
/// player's clock it began.
#[derive(Debug)]
struct Playing {
    /// The song.
    song: Song,
    /// The next of its events to hand to the synthesizer.
    next: usize,
    /// The player's clock, in seconds, at the song's first beat.
    began: f64,
}

/// A soundfont synthesizer playing at most one song, rendering interleaved
/// stereo `f32` into whatever buffer it is handed.
///
/// This is the half of the music a test can drive: no device, no thread and
/// no clock but the samples it has written. [`Music`](crate::Music) is a
/// player on a thread feeding a sound card.
///
/// ```
/// use corvid_music::{Player, Song};
/// # #[cfg(feature = "unison")]
/// # fn main() -> Result<(), corvid_music::Error> {
/// let mut player = Player::new(&corvid_music::Bank::unison()?, 48_000);
/// assert!(!player.playing());
///
/// // Nothing playing is silence, not the last buffer again.
/// let mut out = vec![1.0f32; 2 * 480];
/// player.fill(&mut out)?;
/// assert!(out.iter().all(|sample| *sample == 0.0));
/// # Ok(()) }
/// # #[cfg(not(feature = "unison"))]
/// # fn main() {}
/// ```
pub struct Player {
    /// The soundfont synthesizer over the software mixer.
    synth: Synthesizer<Software>,
    /// The output rate, in hertz.
    rate: u32,
    /// The mixer's clock, in seconds: where the next sample written falls.
    now: f64,
    /// The song, while one is playing.
    playing: Option<Playing>,
}

impl core::fmt::Debug for Player {
    /// Everything but the synthesizer, which does not describe itself.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Player")
            .field("rate", &self.rate)
            .field("now", &self.now)
            .field("playing", &self.playing)
            .finish_non_exhaustive()
    }
}

/// A run of interleaved stereo samples the mixer may render into in place.
struct Window<'a> {
    /// The samples.
    data: &'a mut [f32],
    /// Their rate.
    rate: u32,
}

impl AudioClip for Window<'_> {
    type Value = f32;

    fn data(&self) -> &[f32] {
        self.data
    }

    fn data_mut(&mut self) -> &mut [f32] {
        self.data
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }
}

impl Player {
    /// A player of `bank`'s instruments at `rate` hertz, playing nothing.
    #[must_use]
    pub fn new(bank: &Bank, rate: u32) -> Self {
        let rate = rate.max(1);
        Self {
            synth: Synthesizer::new(bank.sound(), Software::new(rate), rate),
            rate,
            now: 0.0,
            playing: None,
        }
    }

    /// The output rate, in hertz.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }

    /// Whether a song is playing, its last notes' tails included.
    #[must_use]
    pub const fn playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Starts `song` from its beginning, cutting off whatever was playing.
    ///
    /// # Errors
    ///
    /// [`Error::Synth`] if the synthesizer would not release the voices of the
    /// song before.
    pub fn play(&mut self, song: Song) -> Result<(), Error> {
        self.stop()?;
        self.playing = Some(Playing {
            song,
            next: 0,
            began: self.now,
        });
        Ok(())
    }

    /// Stops the song, letting the notes sounding release rather than click.
    ///
    /// # Errors
    ///
    /// [`Error::Synth`] if the synthesizer would not release a voice.
    pub fn stop(&mut self) -> Result<(), Error> {
        self.playing = None;
        self.synth
            .reset(Timestamp::Now)
            .map_err(|why| Error::Synth(why.to_string()))
    }

    /// Writes the next `out.len() / 2` frames of interleaved stereo into
    /// `out`, overwriting it, and moves the clock past them.
    ///
    /// A song that has ended -- its last event played and its tail rung out
    /// -- is no longer [`playing`](Self::playing) after this returns.
    ///
    /// # Errors
    ///
    /// [`Error::Synth`] if the synthesizer refused an event or the render.
    pub fn fill(&mut self, out: &mut [f32]) -> Result<(), Error> {
        let rate = f64::from(self.rate);
        for block in out.chunks_mut(2 * BLOCK) {
            let frames = u32::try_from(block.len() / 2).unwrap_or(0);
            let end = self.now + f64::from(frames) / rate;
            self.dispatch(end)?;
            self.synth
                .render(&mut Window {
                    data: block,
                    rate: self.rate,
                })
                .map_err(|why| Error::Synth(why.to_string()))?;
            self.now = end;
            if self
                .playing
                .as_ref()
                .is_some_and(|p| p.next >= p.song.len() && end >= p.began + p.song.seconds() + TAIL)
            {
                self.playing = None;
            }
        }
        Ok(())
    }

    /// Hands the synthesizer every event of the song due before `end`, each
    /// placed at its own time so the mixer starts it on the right sample.
    fn dispatch(&mut self, end: f64) -> Result<(), Error> {
        let Some(playing) = self.playing.as_mut() else {
            return Ok(());
        };
        while let Some((at, event)) = playing.song.event(playing.next) {
            let when = playing.began + at;
            if when >= end {
                break;
            }
            playing.next += 1;
            self.synth
                .handle(Timestamp::AtSeconds(when.max(self.now)), event)
                .map_err(|why| Error::Synth(why.to_string()))?;
        }
        Ok(())
    }
}
