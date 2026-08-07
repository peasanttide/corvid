//! The sound card, and the one place in this workspace that names `cpal`.
//!
//! # The rule this module is written around
//!
//! The audio callback runs on a thread the operating system owns and schedules
//! against a deadline of a few milliseconds. If it allocates it may hit a
//! global lock; if it waits it may miss the deadline; if it panics it unwinds
//! through a foreign stack frame. So it may do none of the three, and the
//! interesting question is not whether that is asserted but how it is arranged.
//!
//! **It does not allocate** because everything it touches is sized before the
//! stream starts: the [`Mixer`]'s pool is a boxed slice built by
//! [`Mixer::new`], and the queue between the threads is a [`VecDeque`] whose
//! capacity is reserved in [`Audio::open`]. The callback only ever pops from
//! that queue, and popping cannot grow it. The game's thread is the only side
//! that pushes, and it never pushes past the reserved capacity — it drops the
//! oldest note instead. Samples are mixed one at a time straight into the
//! device's buffer, so there is no intermediate to size either.
//!
//! **It does not wait** because it takes the queue with
//! [`try_lock`](Mutex::try_lock) and carries on without it when the game's
//! thread has it. The cost is that a note can be one buffer late, which at the
//! device's own period is a few milliseconds; the alternative is a callback
//! that blocks on a game thread that has been preempted, which is an underrun
//! the player hears as a crack. The other side of the lock is held only for as
//! long as it takes to push a handful of values, so the game's thread waits for
//! microseconds and is the side that may.
//!
//! **It does not panic** because the workspace's lints deny `unwrap`,
//! `expect`, `panic` and `unreachable`, and because the two things those lints
//! do not cover are arranged away by hand: the callback indexes nothing —
//! `indexing_slicing` is *not* one of the workspace's lints, so that is a
//! property of this code rather than something the build enforces — and every
//! arithmetic input is made finite before it crosses over, so there is no
//! overflow for a debug build to trap on.
//!
//! None of that is a proof. It is a set of arrangements that can each be
//! checked by reading the code they name, which is a better thing to write down
//! than an assertion that the callback is real-time safe.

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use corvid_sound::AudioFrame;
use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::catalogue::Catalogue;
use crate::extract::notes;
use crate::heard::Heard;
use crate::mixer::{Mixer, Note};

/// How many voices can sound at once.
///
/// Sixty-four is more than a game with a procedural knock per event will use
/// and small enough that summing them all is a few hundred nanoseconds a
/// sample. Past it the quietest voice is stolen, which
/// [`Mixer::start`](crate::Mixer::start) explains.
const VOICES: usize = 64;

/// How many notes may be waiting for the device to notice them.
///
/// A displayed frame's worth of cues, several times over, so that the game's
/// thread does not reallocate when a device is slow to come back for them.
const QUEUED: usize = 256;

/// How many cue identities a backend remembers.
///
/// At fifteen hertz and a handful of cues a tick this is a couple of seconds of
/// history, which is longer than any rollback this workspace's netcode is meant
/// to produce. [`Heard`] says what happens past it.
const REMEMBERED: usize = 256;

/// Why a device would not play.
///
/// Every case here is the machine rather than the game: an `AudioFrame` cannot
/// be wrong in a way that reaches this type, because it is data.
#[derive(Debug)]
#[non_exhaustive]
pub enum Unavailable {
    /// The platform has no output device. A machine with no sound card, and a
    /// container without one, are both this.
    NoDevice,
    /// A device exists and would not say what it can play.
    NoConfig(cpal::Error),
    /// A device wants a sample format this backend does not write.
    ///
    /// The formats written are `f32`, `f64`, `i16`, `i32`, `u16` and `u8`,
    /// which is every one a desktop or a phone has offered so far. A device
    /// asking for something else is a variant to add rather than a design
    /// decision.
    Unwritable(cpal::SampleFormat),
    /// The device would not open a stream.
    Unopened(cpal::Error),
    /// A stream was opened and would not start.
    Unstarted(cpal::Error),
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => f.write_str("this machine has no audio output device"),
            Self::NoConfig(why) => write!(f, "the device would not say what it can play: {why}"),
            Self::Unwritable(format) => {
                write!(
                    f,
                    "the device wants {format:?} samples, which this backend does not write"
                )
            }
            Self::Unopened(why) => write!(f, "the device would not open a stream: {why}"),
            Self::Unstarted(why) => write!(f, "the stream would not start: {why}"),
        }
    }
}

impl std::error::Error for Unavailable {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoConfig(why) | Self::Unopened(why) | Self::Unstarted(why) => Some(why),
            Self::NoDevice | Self::Unwritable(_) => None,
        }
    }
}

/// What the two threads share: the notes waiting to be started.
type Queue = Arc<Mutex<VecDeque<Note>>>;

/// An open output stream, and everything that decides what goes into it.
///
/// A game builds one of these once and hands it an [`AudioFrame`] per displayed
/// frame.
///
/// ```no_run
/// # fn main() -> Result<(), corvid_audio::Unavailable> {
/// use corvid_audio::{Audio, Catalogue, Timbre};
/// use corvid_sound::{AudioFrame, SoundId};
///
/// const THUD: SoundId = SoundId(2);
///
/// // A machine with no sound card answers `Err`, which is a thing to carry on
/// // without rather than a thing to stop for.
/// let mut audio = Audio::open(Catalogue::new().with(THUD, Timbre::knock(90.0)))?;
///
/// let frame = AudioFrame::new();
/// audio.hear(&frame);
/// # Ok(())
/// # }
/// ```
///
/// # It stays on the thread that opened it
///
/// A `cpal` stream is tied to the thread that built it on some platforms —
/// notably the ones with a COM apartment — and this type is meant to live
/// beside the window loop that produces the frames anyway. So it is deliberately
/// neither [`Send`] nor [`Sync`], and that is arranged rather than inherited:
/// `cpal::Stream` happens to be `Send` on Linux, so a type that merely held one
/// would compile perfectly well when moved to a worker thread and misbehave on
/// somebody else's platform. The [`PhantomData`] below is what makes the
/// compiler refuse it everywhere:
///
/// ```compile_fail
/// fn spawned<T: Send>() {}
/// spawned::<corvid_audio::Audio>();
/// ```
///
/// `tests/mixer.rs` is where that snippet's counterpart lives — the same
/// assertion for a type that *is* `Send`, so the failure above is this type
/// rather than a snippet that could not compile for any reason at all.
pub struct Audio {
    /// The open stream. Dropping it closes the device, which is why it is held
    /// rather than leaked, and why nothing reads it.
    _stream: cpal::Stream,
    /// Where a note is left for the device thread to pick up.
    queue: Queue,
    /// What each sound is described as.
    catalogue: Catalogue,
    /// Which cues have already been started.
    heard: Heard,
    /// The notes of the frame being handed over, kept so a frame with nothing
    /// new in it costs no allocation.
    pending: Vec<Note>,
    /// What the device is running at.
    rate: u32,
    /// How many channels it wants.
    channels: u16,
    /// What keeps this value on the thread that opened it.
    ///
    /// A raw pointer is neither [`Send`] nor [`Sync`], and a zero-sized
    /// [`PhantomData`] of one costs nothing at run time. It is here rather than
    /// left to `cpal::Stream`'s own auto traits because those differ by
    /// platform: on Linux a stream is `Send`, so without this the compiler
    /// would let a game move an `Audio` to a worker thread and only Windows
    /// would find out.
    thread_bound: PhantomData<*const ()>,
}

impl std::fmt::Debug for Audio {
    /// Everything except the stream, which `cpal` does not describe.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("rate", &self.rate)
            .field("channels", &self.channels)
            .field("described", &self.catalogue.len())
            .finish_non_exhaustive()
    }
}

impl Audio {
    /// Opens the platform's default output device and starts playing silence.
    ///
    /// The stream runs from here until this value is dropped. Nothing is heard
    /// until [`hear`](Self::hear) is given a frame with a cue in it.
    ///
    /// # Errors
    ///
    /// [`Unavailable`], every case of which is the machine: no device, a device
    /// that would not describe itself, one wanting a sample format this backend
    /// does not write, or one that would not open or start a stream. A game is
    /// expected to carry on without sound rather than to stop.
    pub fn open(catalogue: Catalogue) -> Result<Self, Unavailable> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(Unavailable::NoDevice)?;
        let supported = device
            .default_output_config()
            .map_err(Unavailable::NoConfig)?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        let channels = config.channels;

        let queue: Queue = Arc::new(Mutex::new(VecDeque::with_capacity(QUEUED)));
        let stream = play(&device, &config, format, Mixer::new(rate, VOICES), &queue)?;
        stream.play().map_err(Unavailable::Unstarted)?;

        tracing::info!(
            name: "corvid_audio.opened",
            rate,
            channels,
            format = ?format,
            "an output device is playing",
        );

        Ok(Self {
            _stream: stream,
            queue,
            catalogue,
            heard: Heard::new(REMEMBERED),
            pending: Vec::with_capacity(QUEUED),
            rate,
            channels,
            thread_bound: PhantomData,
        })
    }

    /// How many samples a second the device is running at.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }

    /// How many channels it is writing.
    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// How many notes have been handed over and not yet picked up.
    ///
    /// Zero, on a device that is running: the callback takes everything it
    /// finds. A number that climbs and stays up is a stream that has stopped
    /// asking for samples, which is the one failure a backend cannot report
    /// through an error — the stream was opened and started, and then the
    /// device went away.
    ///
    /// It answers [`None`] rather than a count when the device thread is
    /// holding the queue at this instant, because the alternative is waiting
    /// for it to let go, and a diagnostic that blocks is worse than one that
    /// says "ask again".
    #[must_use]
    pub fn waiting(&self) -> Option<usize> {
        self.queue.try_lock().ok().map(|queue| queue.len())
    }

    /// Starts whatever in `frame` has not been started already.
    ///
    /// Call it once per displayed frame with the frame the game's `hear`
    /// filled. A cue that was in the last frame and is in this one is not
    /// played twice; [`Heard`] is where that decision and its limits are
    /// written down.
    ///
    /// This runs on the game's thread and may wait for the device thread to
    /// finish reading the queue, which takes as long as popping a few values
    /// takes. That direction is deliberate: the thread with a deadline never
    /// waits for the one without.
    pub fn hear(&mut self, frame: &AudioFrame) {
        // Asked before `notes`, because `Heard::is_new` records the identity it
        // is asked about: extracting first would mark every cue of every
        // subsequent frame as already played and then throw it away, and a
        // poisoned lock never un-poisons, so the silence would be permanent and
        // the record would say the sounds had been heard.
        if self.queue.is_poisoned() {
            // A poisoned lock means the device thread panicked while holding
            // it, which the callback is written not to do. There is nothing
            // useful to do about it here and stopping the game over it would be
            // the wrong trade, so the frame goes unheard and says so.
            tracing::error!(
                name: "corvid_audio.poisoned",
                "the audio queue was left poisoned, so nothing more will be heard",
            );
            return;
        }
        notes(frame, &self.catalogue, &mut self.heard, &mut self.pending);
        if self.pending.is_empty() {
            return;
        }
        let Ok(mut queue) = self.queue.lock() else {
            // Poisoned between the check above and here, so this one frame's
            // cues are marked and unheard. The check catches every frame after
            // it.
            tracing::error!(
                name: "corvid_audio.poisoned",
                "the audio queue was left poisoned, so nothing more will be heard",
            );
            return;
        };
        for note in self.pending.drain(..) {
            if queue.len() >= QUEUED {
                // The device has not come back for these, which means it has
                // stopped. Dropping the oldest keeps the newest sounds, which
                // is the right end to keep.
                queue.pop_front();
            }
            queue.push_back(note);
        }
    }

    /// Forgets every cue identity, so a frame's cues are new sounds again.
    ///
    /// What a game calls when it loads a save or starts a new match: the same
    /// ticks are about to be simulated again and their cues are genuinely
    /// different sounds from the ones already played.
    pub fn restarted(&mut self) {
        self.heard.forget_all();
    }
}

/// Builds a stream in whichever sample format the device asked for.
///
/// One arm per format rather than one generic function, because `cpal`'s
/// `build_output_stream` is generic over the sample type and the format is a
/// runtime value. The body of each arm is the same closure over the same
/// [`Mixer`], and [`fill`] is where it lives.
fn play(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    mixer: Mixer,
    queue: &Queue,
) -> Result<cpal::Stream, Unavailable> {
    /// What `cpal` reports a stream error through. Nothing here can act on one
    /// — the stream is already broken — so it is recorded and the game plays on
    /// in silence.
    fn complained(why: &cpal::Error) {
        tracing::warn!(
            name: "corvid_audio.stream",
            why = %why,
            "the audio device reported an error",
        );
    }

    /// Builds the stream for one concrete sample type.
    macro_rules! build {
        ($sample:ty) => {{
            let mut mixer = mixer;
            let queue = Arc::clone(queue);
            let channels = usize::from(config.channels).max(1);
            device.build_output_stream(
                config.clone(),
                move |out: &mut [$sample], _: &cpal::OutputCallbackInfo| {
                    fill(&mut mixer, &queue, out, channels);
                },
                |why| complained(&why),
                None,
            )
        }};
    }

    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U8 => build!(u8),
        other => return Err(Unavailable::Unwritable(other)),
    };
    stream.map_err(Unavailable::Unopened)
}

/// One callback: take whatever notes are waiting, mix, and convert.
///
/// Every sample is mixed and converted straight into the device's own buffer,
/// so there is no intermediate to size and nothing here to allocate. The only
/// two things this touches that it did not bring with it are the queue, which
/// it takes without waiting and never grows, and the device's buffer, which it
/// walks by iterator.
fn fill<S: cpal::Sample + cpal::FromSample<f32>>(
    mixer: &mut Mixer,
    queue: &Queue,
    out: &mut [S],
    channels: usize,
) {
    if let Ok(mut waiting) = queue.try_lock() {
        while let Some(note) = waiting.pop_front() {
            mixer.start(note);
        }
    }
    for slot in out.chunks_mut(channels) {
        let sample = S::from_sample(mixer.next_sample());
        for out in slot {
            *out = sample;
        }
    }
}
