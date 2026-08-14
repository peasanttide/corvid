//! The lifecycle, and the one trait a game holds.

use serde::{Deserialize, Serialize};

use crate::{Hand, Haptic, Passthrough, Pose, Space, Tracked, Views};

/// The session lifecycle.
///
/// A state machine rather than a set of booleans, because the illegal
/// transitions are the ones that hang a compositor: drawing before the runtime
/// is ready, or carrying on after it has asked to stop.
///
/// ```
/// use corvid_xr::State;
///
/// assert!(State::Idle.may_become(State::Ready));
/// assert!(!State::Idle.may_become(State::Focused));
/// assert!(State::Focused.is_drawing());
/// ```
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum State {
    /// Nothing has been created yet, or a stop has run its course.
    #[default]
    Idle,
    /// Created, not yet drawing.
    Ready,
    /// Drawing, and the headset is on a head.
    Focused,
    /// Drawing, and something else has focus -- a system menu.
    Visible,
    /// The runtime asked us to stop.
    Stopping,
    /// The session is over and the process should let go of it.
    Exiting,
}

impl State {
    /// Every state, in the order they are declared.
    ///
    /// What a test that means to cover the machine iterates, so a state added
    /// later is covered without the test being edited.
    pub const ALL: [Self; 6] = [
        Self::Idle,
        Self::Ready,
        Self::Focused,
        Self::Visible,
        Self::Stopping,
        Self::Exiting,
    ];

    /// Whether the runtime may move from here to `next`.
    ///
    /// Staying put is always legal -- a poll that reports the state again is the
    /// ordinary case. So is leaving for [`Exiting`](Self::Exiting) from
    /// anywhere: a runtime that has lost its instance is not negotiating, and a
    /// state machine that refused the message would leave a game polling a
    /// session that is gone. Everything else is a step along the one path:
    /// `Idle -> Ready -> Visible -> Focused` and back out through `Stopping`.
    #[must_use]
    pub const fn may_become(self, next: Self) -> bool {
        if self as u8 == next as u8 {
            return true;
        }
        match self {
            Self::Idle => matches!(next, Self::Ready | Self::Exiting),
            Self::Ready => matches!(next, Self::Visible | Self::Stopping | Self::Exiting),
            Self::Visible => matches!(next, Self::Focused | Self::Stopping | Self::Exiting),
            Self::Focused => matches!(next, Self::Visible | Self::Stopping | Self::Exiting),
            Self::Stopping => matches!(next, Self::Idle | Self::Exiting),
            Self::Exiting => false,
        }
    }

    /// Whether a frame should be drawn in this state.
    #[must_use]
    #[inline]
    pub const fn is_drawing(self) -> bool {
        matches!(self, Self::Focused | Self::Visible)
    }

    /// Whether the session is over.
    #[must_use]
    #[inline]
    pub const fn is_over(self) -> bool {
        matches!(self, Self::Exiting)
    }
}

/// Which hand.
///
/// The trait below indexes hands by `usize` because that is what an array of
/// two is indexed by; this is the name for the two values that are in range.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Side {
    /// Index `0`.
    #[default]
    Left,
    /// Index `1`.
    Right,
}

impl Side {
    /// Both, left first.
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];

    /// Where this hand sits in an array of two.
    #[must_use]
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The other one.
    #[must_use]
    #[inline]
    pub const fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl From<Side> for usize {
    #[inline]
    fn from(side: Side) -> Self {
        side.index()
    }
}

/// A `usize` that is not `0` or `1` names no hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, thiserror::Error)]
#[error("a hand is 0 (left) or 1 (right)")]
pub struct NotAHand;

impl TryFrom<usize> for Side {
    type Error = NotAHand;

    /// # Errors
    ///
    /// [`NotAHand`] for anything but `0` and `1`.
    #[inline]
    fn try_from(index: usize) -> Result<Self, Self::Error> {
        match index {
            0 => Ok(Self::Left),
            1 => Ok(Self::Right),
            _ => Err(NotAHand),
        }
    }
}

/// What a game holds. One per process.
///
/// Implemented twice: by [`ScriptedHeadset`](crate::ScriptedHeadset), which is
/// a recording and needs no hardware, and -- behind the `openxr` feature -- by
/// [`OpenXr`](crate::runtime::OpenXr), which is a runtime. Everything a game
/// writes against this trait runs in CI.
pub trait Headset: Send {
    /// Advance the session and report where it now is.
    ///
    /// The one method that mutates the lifecycle. Everything else reports.
    fn poll(&mut self) -> State;

    /// Where the head is, in the named space.
    fn head(&self, space: Space) -> Tracked<Pose>;

    /// The two eyes, for this frame's predicted display time.
    fn views(&self) -> Tracked<Views>;

    /// Both hands, left first.
    fn hands(&self) -> [Tracked<Hand>; 2];

    /// Fire a haptic effect on one hand.
    ///
    /// An index that names no hand does nothing, because a rumble is a
    /// courtesy and refusing one is not worth a `Result`.
    fn rumble(&mut self, hand: usize, effect: Haptic);

    /// Whether passthrough is available, and whether it is on.
    fn passthrough(&self) -> Passthrough;

    /// Ask for passthrough. [`Passthrough::Unavailable`] is a normal answer.
    fn set_passthrough(&mut self, on: bool) -> Passthrough;

    /// How many display frames per second the runtime intends.
    fn rate(&self) -> u16;

    /// One hand, by side.
    ///
    /// Provided, because it is [`hands`](Self::hands) indexed and there is
    /// nothing a runtime could do differently.
    fn hand(&self, side: Side) -> Tracked<Hand> {
        self.hands()[side.index()]
    }
}
