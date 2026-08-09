//! The frame itself, and the ears it is written for.

use alloc::vec::Vec;

use crate::{Bus, Cue, CueId, Source};
use corvid_fixed::Factor16;
use corvid_time::Tick;
use corvid_transform::GlobalFineTransform;

/// Where the ears are.
///
/// The pose is in world space, and it is the frame of reference every
/// [`Source`] and [`Cue`] position in the same [`AudioFrame`] is expressed
/// relative to. A backend mixing this frame needs the offsets and not the pose;
/// the pose is here so that a captured frame can be placed back in the world it
/// came from, and so that a backend doing room acoustics has the one piece the
/// offsets cannot reconstruct.
///
/// Keeping the offsets listener-relative rather than absolute is what keeps a
/// [`FinePoint`](corvid_vector::FinePoint) wide enough. An absolute position
/// on an earth-scale world needs the 64-bit
/// [`GlobalFinePoint`](corvid_vector::GlobalFinePoint) that the pose uses; a
/// relative one for anything audible fits ±32.7 km at 15.26 µm with room to
/// spare, and it means a frame recorded a hundred kilometres from the origin is
/// the same bytes as the one recorded at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Listener {
    /// Where the ears are in the world, and which way they face.
    pub pose: GlobalFineTransform,
    /// The gain applied to everything, after the buses.
    pub gain: Factor16,
}

impl Listener {
    /// Ears at `pose`, hearing everything.
    ///
    /// The pose is stored exactly as given and is in world space; it is
    /// [`default`](Self::default) that puts the ears at the origin facing the
    /// identity pose's forward.
    #[must_use]
    #[inline]
    pub const fn new(pose: GlobalFineTransform) -> Self {
        Self {
            pose,
            gain: Factor16::ONE,
        }
    }

    /// Sets the gain applied to everything.
    #[must_use]
    #[inline]
    pub const fn with_gain(self, gain: Factor16) -> Self {
        Self { gain, ..self }
    }
}

impl Default for Listener {
    /// The identity pose at full gain.
    ///
    /// Not derived. A derived [`Factor16`] is [`ZERO`](Factor16::ZERO), and a
    /// default listener that hears nothing would make a frame that was built
    /// but never given a listener silent rather than centred — which is a bug
    /// that reaches a player as "the audio stopped working" rather than as a
    /// red test.
    #[inline]
    fn default() -> Self {
        Self::new(GlobalFineTransform::IDENTITY)
    }
}

/// Everything to hear this frame, and nothing that could turn it into sound.
///
/// # Every position in here is relative to the listener
///
/// This is the one thing to know before writing anything that fills a frame.
/// [`listener.pose`](Listener::pose) is in world space; every [`Source`] and
/// [`Cue`] position is an **offset in the listener's own frame**, and there is
/// no world-space position on either of them. So an extractor —
/// `Auralizer::hear` — is given the ears *and* the sounds, and does the
/// subtraction and the rotation itself. A `hear` written as though a source
/// carried a world
/// position would compile against these types and be wrong everywhere but the
/// origin, which is exactly where it would be tested.
///
/// The reason is width. A [`FinePoint`](corvid_vector::FinePoint) is 32 bits
/// an axis and reaches ±32.7 km at 15.26 µm, which is far past anything
/// audible; an absolute position on an earth-scale world needs the 64-bit
/// [`GlobalFinePoint`](corvid_vector::GlobalFinePoint) that the pose uses. It
/// also means a frame recorded a hundred kilometres from the origin is
/// byte-identical to one recorded at it, so a capture does not get noisier the
/// further a session wanders.
///
/// # What this is
///
/// This is the whole of what `corvid_sound` is: a value that says what the game
/// wants heard, with no device, no voices, no sample buffer and no mixer behind
/// it. Turning it into samples is a backend's job — a device-native spatializer
/// in production, where the platform's own HRTF is better than anything this
/// workspace would write, and a small reference mixer in a headless run, which
/// writes bit-identical WAV. `corvid_audio` is the production-side one in this
/// workspace; there is no reference mixer, so nothing here is compared as a
/// waveform.
///
/// What that buys is that the *frame* is the artefact goldens compare. Nothing
/// in it is a float, which removes the usual reason two machines that computed
/// the same thing disagree about the bytes, and a capture taken today diffs
/// against one taken by last month's build. Waveforms are only ever compared
/// against the reference mixer, so the honest consequence is that a WAV golden
/// validates the frame and the reference mixer, and never the production audio
/// path.
///
/// ```
/// use corvid_sound::{AudioFrame, Cue, SoundId, Source, SourceId};
/// use corvid_time::Tick;
///
/// const THUD: SoundId = SoundId(1);
///
/// let mut frame = AudioFrame::new();
/// frame.source(Source::new(SourceId(0), SoundId(4)));
///
/// // Two bounces on one tick. `next_id` numbers the second after the first,
/// // so they are two cues and not one heard twice.
/// let first = frame.next_id(Tick(97));
/// frame.cue(Cue::new(first, THUD));
/// let second = frame.next_id(Tick(97));
/// frame.cue(Cue::new(second, THUD));
///
/// assert_eq!(first.serial, 0);
/// assert_eq!(second.serial, 1);
/// assert_ne!(first, second);
///
/// // A runtime is meant to hold one frame forever and refill it, so `clear`
/// // keeps the capacity and drops the contents.
/// frame.clear();
/// assert!(frame.is_empty());
/// assert_eq!(frame.next_id(Tick(97)).serial, 0);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct AudioFrame {
    /// Where the ears are, and the frame everything below is relative to.
    pub listener: Listener,
    /// The sounds that are playing.
    pub sources: Vec<Source>,
    /// The one-shots fired.
    pub cues: Vec<Cue>,
    /// The mixing buses.
    pub buses: Vec<Bus>,
}

impl AudioFrame {
    /// An empty frame with the default listener.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            listener: Listener::new(GlobalFineTransform::IDENTITY),
            sources: Vec::new(),
            cues: Vec::new(),
            buses: Vec::new(),
        }
    }

    /// Empties the frame and resets the listener, keeping every allocation.
    ///
    /// A runtime is meant to hold one frame for the life of the process and
    /// hand it to the extractor once per displayed frame, so this would run at
    /// display rate and must not allocate. `Vec::clear` keeps capacity;
    /// assigning a fresh `Vec` would not, and `tests/frame.rs` is what holds
    /// that in place. It checks the capacity across a clear-and-refill cycle,
    /// which is evidence about these three vectors and not a proof that nothing
    /// anywhere in an extraction allocates.
    #[inline]
    pub fn clear(&mut self) {
        self.listener = Listener::default();
        self.sources.clear();
        self.cues.clear();
        self.buses.clear();
    }

    /// Returns `true` when there is nothing to hear.
    ///
    /// The listener is not part of the answer: a frame with a listener and no
    /// sound in it is silent whatever the ears are doing.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.cues.is_empty()
    }

    /// Places the ears.
    #[inline]
    pub const fn listen(&mut self, listener: Listener) {
        self.listener = listener;
    }

    /// Appends a playing sound.
    #[inline]
    pub fn source(&mut self, source: Source) {
        self.sources.push(source);
    }

    /// Appends a one-shot.
    ///
    /// Nothing here checks that its [`CueId`] is unused, or that it is one
    /// [`next_id`](Self::next_id) would have handed out. Both are obligations
    /// on the caller. Taking the identity rather than inventing one is what a
    /// caller replaying a captured frame needs: it has identities it must
    /// reproduce rather than mint.
    #[inline]
    pub fn cue(&mut self, cue: Cue) {
        self.cues.push(cue);
    }

    /// Appends a mixing bus.
    #[inline]
    pub fn bus(&mut self, bus: Bus) {
        self.buses.push(bus);
    }

    /// The identity to give the next cue fired on `fired`: that tick, and one
    /// past the highest serial already in this frame for it.
    ///
    /// It reads the frame and holds no counter of its own, which is what makes
    /// the numbering reproducible from a serialized frame alone — a tool that
    /// loads a capture and appends to it gets the same answer the extractor
    /// got. The cost is a scan of the cue list per call, which is linear in a
    /// list that holds the one-shots of a single display frame.
    ///
    /// It is *not* a reservation. Two calls with no [`cue`](Self::cue) between
    /// them return the same identity, and pushing both cues gives a frame with
    /// a repeated identity that nothing here rejects.
    ///
    /// # At the ceiling
    ///
    /// A serial is a `u16`. A frame carrying 65 536 cues fired on one tick has
    /// no unused serial left for that tick, and this returns
    /// [`u16::MAX`] again rather than wrapping to zero or panicking — the
    /// workspace denies `panic`, and wrapping would collide with the *first*
    /// cue rather than the last. A frame in that state is already telling a
    /// mixer something untrue, and the honest reading is that 65 536 one-shots
    /// in one tick is a bug upstream.
    #[must_use]
    #[inline]
    pub fn next_id(&self, fired: Tick) -> CueId {
        let mut serial = 0;
        for cue in &self.cues {
            if cue.id.fired == fired {
                serial = serial.max(cue.id.serial.saturating_add(1));
            }
        }
        CueId { fired, serial }
    }
}
