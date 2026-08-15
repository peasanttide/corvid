//! Where a built-in track puts the head and the hands on one frame.
//!
//! The seam against `mod.rs` is that nothing here is a recording: this is the
//! arithmetic the three synthesised tracks are generated from, so a test that
//! wants a moving headset needs no file.

use corvid_fixed::{Angle32, Factor16, Pitch32};
use corvid_rotation::{FineRotation, Versor};

use crate::State;

/// Where a built-in track's head and hands are on one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Motion {
    /// Which way the head faces.
    pub(super) yaw: Angle32,
    /// How far the right hand has swept round from the head's facing.
    pub(super) reach: Angle32,
    /// How closed the right fist is.
    pub(super) grip: Factor16,
    /// How closed the right pinch is.
    pub(super) pinch: Factor16,
}

/// The whole of a session, in order: what a track walks through.
///
/// A built-in track records a session rather than a burst of frames, so it
/// opens and closes the way a runtime does. Every prefix of this path is a
/// legal walk from [`State::Idle`], which is what lets a track of three frames
/// be as well-formed as one of nine hundred.
const PATH: [State; 6] = [
    State::Ready,
    State::Visible,
    State::Focused,
    State::Visible,
    State::Stopping,
    State::Idle,
];

/// How many frames a track needs before it has room for a whole session.
const WHOLE: u32 = 6;

/// Which state frame `index` of a `total`-frame built-in track is in.
///
/// Two frames open the session and three close it; everything between them is
/// [`State::Focused`], which is the part a game draws.
pub(super) fn state_at(index: u32, total: u32) -> State {
    if total < WHOLE {
        return PATH.get(index as usize).copied().unwrap_or(State::Focused);
    }
    match index {
        0 => State::Ready,
        1 => State::Visible,
        i if i + 3 == total => State::Visible,
        i if i + 2 == total => State::Stopping,
        i if i + 1 == total => State::Idle,
        _ => State::Focused,
    }
}

/// A yaw as a packed rotation, level and unrolled.
pub(super) const fn turn(yaw: Angle32) -> FineRotation {
    FineRotation::from_versor(Versor::from_yaw_pitch_roll(
        yaw,
        Pitch32::ZERO,
        Angle32::ZERO,
    ))
}
