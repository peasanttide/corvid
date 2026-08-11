//! The controls that carry a quantity rather than a name.

use core::fmt;

/// One two-axis continuous control on a device.
///
/// **Two kinds, and which one a control is decides how a game must read it.**
/// The first two are *relative*: what arrives is how far something moved since
/// the last report, and a frame in which the platform said nothing is a frame in
/// which nothing moved. The three below are *absolute*: what arrives is where
/// the control is being held, which is still true on a frame the platform did
/// not mention it.
///
/// [`Reading`](crate::platform::Reading) is what a binding says about that, and
/// getting it wrong is silent either way: a stick bound as a displacement is a
/// control that works only while it is moving, and a mouse bound as a deflection
/// turns the camera by the square of the frame time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Axis {
    /// How far the mouse moved, in whatever unit the platform reports motion
    /// in -- pixels on every desktop this workspace builds for.
    MouseMotion,
    /// How far the wheel turned, in whatever unit the platform reports a
    /// detent in.
    Scroll,
    /// How far the left stick is pushed, positive right and up.
    ///
    /// A *level* and not a motion, which is the whole difference between a
    /// stick and a mouse: it says where the stick is being held, so it answers
    /// on [`Input::analog`](crate::Input::analog) through a
    /// [`Reading::Deflection`](crate::platform::Reading) binding and the
    /// frame's `dt` multiplies it.
    LeftStick,
    /// The right stick, the same way.
    RightStick,
    /// How far the two triggers are pulled: `x` is the left one and `y` is the
    /// right.
    ///
    /// Two one-dimensional controls in one two-dimensional axis, because they
    /// are read together and reported together by every platform, and two
    /// variants would each waste a component. A game that wants one binds this
    /// and reads the component it cares about.
    ///
    /// Both run `0.0 ..= 1.0` rather than either side of zero: a trigger at
    /// rest is at rest, not centred.
    Triggers,
}

impl Axis {
    /// Every axis this vocabulary names, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::MouseMotion,
        Self::Scroll,
        Self::LeftStick,
        Self::RightStick,
        Self::Triggers,
    ];

    /// The name this axis is written down under.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MouseMotion => "MouseMotion",
            Self::Scroll => "Scroll",
            Self::LeftStick => "LeftStick",
            Self::RightStick => "RightStick",
            Self::Triggers => "Triggers",
        }
    }

    /// The axis a name denotes, or [`None`] if this vocabulary does not name
    /// it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "MouseMotion" => Some(Self::MouseMotion),
            "Scroll" => Some(Self::Scroll),
            "LeftStick" => Some(Self::LeftStick),
            "RightStick" => Some(Self::RightStick),
            "Triggers" => Some(Self::Triggers),
            _ => None,
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
