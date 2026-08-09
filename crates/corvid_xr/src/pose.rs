//! What a runtime reports, and how much of it to believe.
//!
//! Every value a headset hands back arrives wrapped in [`Tracked`], because a
//! runtime that has lost a hand still has an opinion about where it was. The
//! wrapper is what keeps "I do not know where this is" apart from "it is here",
//! which are the two things a bare transform cannot tell apart.

use core::time::Duration;

use corvid_transform::FineTransform;
use serde::{Deserialize, Serialize};

/// A pose in stage space: metres from the stage's origin.
///
/// The fine tier, because this is where the precision work earns itself. A
/// [`GlobalPoint`](corvid_vector::GlobalPoint)'s 3.9 mm is finer than
/// anything a cursor can pick and coarser than the shimmer a wearer sees on
/// every frame; [`GlobalFinePoint`](corvid_vector::GlobalFinePoint)'s
/// 15.26 µm is not.
pub type Pose = FineTransform;

/// How much a tracked value is to be believed.
///
/// Ordered, so `confidence >= Confidence::Inferred` is the usual test and it
/// reads the way it sounds.
///
/// ```
/// use corvid_xr::Confidence;
///
/// assert!(Confidence::Tracked > Confidence::Inferred);
/// assert!(Confidence::Inferred > Confidence::Lost);
/// assert_eq!(Confidence::default(), Confidence::Lost);
/// ```
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Confidence {
    /// Nothing is being tracked; the value is the last one that was.
    #[default]
    Lost,
    /// Predicted from something else — a hand behind the back, from the arm.
    Inferred,
    /// Measured.
    Tracked,
}

impl Confidence {
    /// Whether the value is worth drawing at all.
    ///
    /// True for [`Inferred`](Self::Inferred) as well as
    /// [`Tracked`](Self::Tracked): a hand predicted from an arm is in roughly
    /// the right place, and a game that refuses to draw it is a game whose
    /// hands vanish when they go behind the player's back.
    #[must_use]
    #[inline]
    pub const fn is_believed(self) -> bool {
        matches!(self, Self::Inferred | Self::Tracked)
    }
}

impl From<Confidence> for bool {
    /// [`Confidence::is_believed`].
    #[inline]
    fn from(confidence: Confidence) -> Self {
        confidence.is_believed()
    }
}

/// A value the runtime reports, with how much it is to be believed and when it
/// was true.
///
/// A hand behind the player's back is [`Confidence::Inferred`] and a controller
/// on a table is [`Confidence::Lost`], and both still carry a last-known value.
/// Returning an `Option` would make every call site choose between a jump to the
/// origin and its own memory of where the hand was; this lets a game fade a hand
/// out instead.
///
/// **A game that ignores [`confidence`](Self::confidence) gets a hand frozen
/// where tracking failed.** That is the better default of the two: a frozen hand
/// reads as a tracking glitch, and a hand at the origin reads as a bug in the
/// game.
///
/// ```
/// use core::time::Duration;
/// use corvid_xr::{Confidence, Pose, Tracked};
///
/// let lost = Tracked::new(Pose::IDENTITY, Confidence::Lost, Duration::from_millis(11));
/// assert_eq!(lost.believed(), None);
/// // The last-known value is still there, for a game that wants to fade it.
/// assert_eq!(lost.value, Pose::IDENTITY);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tracked<T> {
    /// The value, believed or not.
    pub value: T,
    /// How much of it to believe.
    pub confidence: Confidence,
    /// When the runtime says this was true, since the session began.
    pub at: Duration,
}

impl<T> Tracked<T> {
    /// A value with a stated confidence and time.
    #[must_use]
    #[inline]
    pub const fn new(value: T, confidence: Confidence, at: Duration) -> Self {
        Self {
            value,
            confidence,
            at,
        }
    }

    /// A measured value.
    #[must_use]
    #[inline]
    #[expect(
        clippy::self_named_constructors,
        reason = "the three constructors are named for the three confidences, and this type is named after the strongest of them; renaming one to satisfy the lint would break the trio"
    )]
    pub const fn tracked(value: T, at: Duration) -> Self {
        Self::new(value, Confidence::Tracked, at)
    }

    /// A value predicted from something else.
    #[must_use]
    #[inline]
    pub const fn inferred(value: T, at: Duration) -> Self {
        Self::new(value, Confidence::Inferred, at)
    }

    /// A last-known value nothing is measuring any more.
    #[must_use]
    #[inline]
    pub const fn lost(value: T, at: Duration) -> Self {
        Self::new(value, Confidence::Lost, at)
    }

    /// The value, if it is worth trusting at all.
    ///
    /// `None` on [`Confidence::Lost`] and nothing else.
    #[must_use]
    #[inline]
    pub fn believed(self) -> Option<T> {
        if self.confidence.is_believed() {
            Some(self.value)
        } else {
            None
        }
    }

    /// Whether the value was measured rather than predicted or remembered.
    #[must_use]
    #[inline]
    pub const fn is_tracked(&self) -> bool {
        matches!(self.confidence, Confidence::Tracked)
    }

    /// The same reading with the value replaced, keeping the confidence and the
    /// time.
    ///
    /// What a game uses to carry a pose through a conversion without deciding
    /// again how much to believe it.
    #[must_use]
    #[inline]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Tracked<U> {
        Tracked {
            value: f(self.value),
            confidence: self.confidence,
            at: self.at,
        }
    }

    /// The same reading, lowered to at most `ceiling`.
    ///
    /// A value derived from two readings is worth no more than the weaker of
    /// them, and this is how that is said.
    #[must_use]
    #[inline]
    pub fn capped(mut self, ceiling: Confidence) -> Self {
        if self.confidence > ceiling {
            self.confidence = ceiling;
        }
        self
    }
}

/// Which reference space a pose is in.
///
/// The workspace's axes throughout: **+X** right, **+Y** forward, **+Z** up, so
/// a stage pose composes with a world transform without a change of basis.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Space {
    /// The floor under the player. What a room-scale game wants.
    #[default]
    Stage,
    /// Where the head was when the session began. What a seated game wants.
    Local,
    /// The head itself, which is what a HUD attached to the face uses.
    View,
}
