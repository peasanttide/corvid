//! A sound that is already playing.

use corvid_fixed::{Factor16, I8F8};

use crate::{BusId, SoundId, SourceId};
use corvid_vector::FinePoint;

/// A continuously playing sound: where it is, how loud, how fast, how muffled,
/// and where it is routed.
///
/// A source is present in every frame for as long as it is audible. Dropping it
/// out of the list is how a frame says the sound has stopped, and the backend
/// decides whether that means cutting the voice or releasing it — this crate
/// carries no envelope and no fade.
///
/// The position is an offset **in the listener's own frame**, in the
/// workspace's right-handed +X right, +Y forward, +Z up convention. A
/// [`FinePoint`] reaches ±32.7 km at 15.26 µm, which is far past anything
/// audible; a source further away than that cannot be expressed and narrowing
/// the world-space offset into one is the extractor's job, not this crate's.
///
/// ```
/// use corvid_fixed::{Factor16, I8F8};
/// use corvid_sound::{BusId, SoundId, Source, SourceId};
/// use corvid_vector::finepoint;
///
/// const TORCH: SoundId = SoundId(4);
///
/// let torch = Source::new(SourceId(1), TORCH)
///     .at(finepoint(2, 3, 0))
///     .with_gain(Factor16::from_f64(0.75))
///     .occluded_by(Factor16::from_f64(0.25));
///
/// // What was not asked for is neutral: unity pitch, master bus, unoccluded.
/// assert_eq!(Source::new(SourceId(1), TORCH).pitch, I8F8::ONE);
/// assert_eq!(Source::new(SourceId(1), TORCH).bus, BusId::MASTER);
/// assert_eq!(Source::new(SourceId(1), TORCH).occlusion, Factor16::ZERO);
/// # assert_eq!(torch.gain, Factor16::from_f64(0.75));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Source {
    /// Which voice this is, from one frame to the next.
    pub id: SourceId,
    /// Which recording is playing.
    pub sound: SoundId,
    /// Which bus it is routed through.
    pub bus: BusId,
    /// Where it is, as an offset in the listener's frame.
    pub position: FinePoint,
    /// How loud, before distance and before the bus.
    pub gain: Factor16,
    /// Playback rate, where [`I8F8::ONE`] is the recorded rate.
    pub pitch: I8F8,
    /// How much is in the way, where [`Factor16::ZERO`] is clear line of sight.
    ///
    /// Deliberately a number and not a filter. What a backend does with it —
    /// a low-pass, an attenuation, a reverb send, nothing at all — is the
    /// backend's decision, and two backends may reasonably differ.
    pub occlusion: Factor16,
}

impl Source {
    /// A source at the listener, at full gain, unoccluded, on the master bus,
    /// playing at the recorded rate.
    #[must_use]
    #[inline]
    pub const fn new(id: SourceId, sound: SoundId) -> Self {
        Self {
            id,
            sound,
            bus: BusId::MASTER,
            position: FinePoint::ZERO,
            gain: Factor16::ONE,
            pitch: I8F8::ONE,
            occlusion: Factor16::ZERO,
        }
    }

    /// Places it, as an offset in the listener's frame.
    #[must_use]
    #[inline]
    pub const fn at(self, position: FinePoint) -> Self {
        Self { position, ..self }
    }

    /// Routes it through `bus`.
    #[must_use]
    #[inline]
    pub const fn on(self, bus: BusId) -> Self {
        Self { bus, ..self }
    }

    /// Sets the gain.
    #[must_use]
    #[inline]
    pub const fn with_gain(self, gain: Factor16) -> Self {
        Self { gain, ..self }
    }

    /// Sets the playback rate.
    #[must_use]
    #[inline]
    pub const fn with_pitch(self, pitch: I8F8) -> Self {
        Self { pitch, ..self }
    }

    /// Sets how much is in the way.
    #[must_use]
    #[inline]
    pub const fn occluded_by(self, occlusion: Factor16) -> Self {
        Self { occlusion, ..self }
    }
}
