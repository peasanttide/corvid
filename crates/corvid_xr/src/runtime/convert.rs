//! `OpenXR`'s own numbers as the values this crate holds.
//!
//! The seam is that nothing here has any state. Every function is a pure
//! reading of one runtime value, which is what keeps the session code in
//! `mod.rs` about *when* things happen rather than about what they mean.

use corvid_fixed::{I2F30, I48F16};
use corvid_rotation::{FineRotation, Versor};
use corvid_vector::GlobalFinePoint;

use crate::{Confidence, Pose, State};

/// An application name clipped to what the runtime's buffer holds.
pub(super) fn clipped(name: &str) -> &str {
    const ROOM: usize = 127;
    if name.len() <= ROOM {
        return name;
    }
    let mut end = ROOM;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// The lifecycle state a runtime's session state means here.
pub(super) const fn lifecycle(state: openxr::SessionState) -> State {
    match state {
        openxr::SessionState::READY => State::Ready,
        openxr::SessionState::SYNCHRONIZED | openxr::SessionState::VISIBLE => State::Visible,
        openxr::SessionState::FOCUSED => State::Focused,
        openxr::SessionState::STOPPING => State::Stopping,
        openxr::SessionState::LOSS_PENDING | openxr::SessionState::EXITING => State::Exiting,
        _ => State::Idle,
    }
}

/// How much a located pose is to be believed.
pub(super) fn believed(flags: openxr::SpaceLocationFlags) -> Confidence {
    if flags.contains(
        openxr::SpaceLocationFlags::POSITION_TRACKED
            | openxr::SpaceLocationFlags::ORIENTATION_TRACKED,
    ) {
        Confidence::Tracked
    } else if flags.intersects(
        openxr::SpaceLocationFlags::POSITION_VALID | openxr::SpaceLocationFlags::ORIENTATION_VALID,
    ) {
        Confidence::Inferred
    } else {
        Confidence::Lost
    }
}

/// The same, for a view state.
pub(super) fn seen(flags: openxr::ViewStateFlags) -> Confidence {
    if flags.contains(
        openxr::ViewStateFlags::POSITION_TRACKED | openxr::ViewStateFlags::ORIENTATION_TRACKED,
    ) {
        Confidence::Tracked
    } else if flags.intersects(
        openxr::ViewStateFlags::POSITION_VALID | openxr::ViewStateFlags::ORIENTATION_VALID,
    ) {
        Confidence::Inferred
    } else {
        Confidence::Lost
    }
}

/// An `OpenXR` pose in this workspace's axes.
///
/// `OpenXR` is **+X** right, **+Y** up, **-Z** forward; this workspace is **+X**
/// right, **+Y** forward, **+Z** up. The two differ by a quarter turn about
/// **X**, which is a proper rotation -- so a position's components swap and one
/// negates, and a quaternion's vector part does the same while its scalar is
/// left alone.
pub(super) fn pose(from: openxr::Posef) -> Pose {
    let position = GlobalFinePoint::new(
        I48F16::from_f64(f64::from(from.position.x)),
        I48F16::from_f64(f64::from(-from.position.z)),
        I48F16::from_f64(f64::from(from.position.y)),
    );
    let turn = |value: f32| I2F30::from_f64(f64::from(value));
    let rotation = Versor::from_xyzw(
        turn(from.orientation.x),
        turn(-from.orientation.z),
        turn(from.orientation.y),
        turn(from.orientation.w),
    )
    .map_or(FineRotation::IDENTITY, FineRotation::from_versor);
    Pose::new(position, rotation)
}

/// A runtime's zero-to-one analogue value as a [`Factor16`](corvid_fixed::Factor16).
pub(super) fn factor(value: f32) -> corvid_fixed::Factor16 {
    corvid_fixed::Factor16::from_f64(f64::from(value.clamp(0.0, 1.0)))
}
