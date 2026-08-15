//! Raw platform numbers as the values Corvid's input snapshot holds.
//!
//! The seam against `driver.rs` is that nothing here has any state: an icon, a
//! scroll delta and a pointer position are each a function of their arguments,
//! which is what lets the pointer arithmetic be tested with no window open.

use corvid_fixed::Signed16;
use corvid_input::Analog;
use corvid_render::Icon;

use crate::state::Size;

/// A Corvid icon as the one `winit` takes.
///
/// [`None`] where the platform will not have it, which is what `winit` answers
/// for an icon it cannot build -- and the window opens with the platform's own
/// icon rather than not opening, because a picture is not a reason to refuse
/// somebody a game. The refusal is said out loud, since an icon that silently
/// did not appear is a thing nobody can debug from the outside.
pub(super) fn icon(icon: &Icon) -> Option<winit::window::Icon> {
    match winit::window::Icon::from_rgba(icon.to_bytes(), icon.width(), icon.height()) {
        Ok(built) => Some(built),
        Err(why) => {
            tracing::warn!(
                name: "corvid_window.icon",
                width = icon.width(),
                height = icon.height(),
                why = %why,
                "this platform would not take the game's icon, so the window \
                 opens with the platform's own",
            );
            None
        }
    }
}

/// A platform's `f64` delta as the integer device units `corvid_input` counts.
///
/// Truncating towards zero rather than rounding, so that a stream of small
/// sub-unit deltas does not accumulate into motion the player did not make. A
/// finite delta too large for an `i32` clamps; one that is not finite at all is
/// no movement, because a device that reports a `NaN` has malfunctioned rather
/// than moved a very long way.
#[expect(
    clippy::cast_possible_truncation,
    reason = "this is the boundary between a platform's f64 deltas and the integer units a binding table divides; the clamp above the cast keeps the value inside an i32, and a delta that large is a device fault rather than a movement"
)]
pub(super) fn round(value: f64) -> i32 {
    if value.is_finite() {
        value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    } else {
        0
    }
}

/// Where the pointer is, as a value from `-1.0` to `1.0` on each axis.
///
/// `x` runs left to right and `y` runs bottom to top, which is the convention
/// [`Analog`] documents and the opposite of the one a window reports in.
pub(super) fn pointer(x: f64, y: f64, size: Size) -> Option<Analog> {
    if size.is_empty() {
        return None;
    }
    let across = 2.0 * x / f64::from(size.width) - 1.0;
    let down = 2.0 * y / f64::from(size.height) - 1.0;
    Some(Analog::new(
        Signed16::from_f64(across.clamp(-1.0, 1.0)),
        Signed16::from_f64((-down).clamp(-1.0, 1.0)),
    ))
}

#[cfg(test)]
mod tests {
    //! The three rules that have no window in them.
    //!
    //! Everything else in this module needs an event loop, and an event loop
    //! needs a display server. `tests/` says which claims about this crate are
    //! therefore checked by hand rather than by `cargo test`.

    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
    )]

    use super::{Signed16, Size, pointer, round};
    use corvid_input::Cursor;

    use crate::run::driver::allowed;
    use crate::state::SurfaceState;

    #[test]
    fn the_pointer_is_centred_in_the_middle_and_y_runs_upwards() {
        // The y flip is the one that is invisible until somebody aims with the
        // mouse: a window reports downwards from the top and `Analog` is
        // documented as positive up. Both corners are asserted, because a flip
        // that was missed puts the top corner where the bottom one belongs and
        // a single corner would still be at the right distance from the middle.
        let size = Size::new(800, 600);
        assert_eq!(pointer(400.0, 300.0, size), Some(super::Analog::ZERO));

        let top_left = pointer(0.0, 0.0, size).unwrap();
        assert_eq!(top_left.x, Signed16::MIN);
        assert_eq!(top_left.y, Signed16::MAX);

        let bottom_right = pointer(800.0, 600.0, size).unwrap();
        assert_eq!(bottom_right.x, Signed16::MAX);
        assert_eq!(bottom_right.y, Signed16::MIN);
    }

    #[test]
    fn a_pointer_outside_the_window_clamps_and_a_window_with_no_area_has_none() {
        // A platform reports a position outside the window during a drag, and
        // an unclamped one would wrap through `Signed16` into the opposite
        // corner.
        let size = Size::new(800, 600);
        assert_eq!(pointer(-500.0, -500.0, size).unwrap().x, Signed16::MIN);
        assert_eq!(pointer(5000.0, 5000.0, size).unwrap().x, Signed16::MAX);
        assert_eq!(pointer(10.0, 10.0, Size::new(0, 600)), None);
    }

    /// A window nobody is looking at cannot hold the pointer.
    ///
    /// Headless on purpose: the rule is a function of two booleans, and a test
    /// that opened a window to check it would be a test most machines skip. The
    /// window-opening one beside it -- `tests/cursor.rs` -- is about what the
    /// *platform* does with a request, which is the other half and cannot be
    /// checked this way.
    #[test]
    fn an_unfocused_or_hidden_window_may_not_hold_the_pointer() {
        let looking = SurfaceState {
            focused: true,
            occluded: false,
            ..SurfaceState::default()
        };
        let away = SurfaceState {
            focused: false,
            ..looking
        };
        let hidden = SurfaceState {
            occluded: true,
            ..looking
        };

        for wanted in [
            Cursor::Free,
            Cursor::Hidden,
            Cursor::Confined,
            Cursor::Locked,
        ] {
            // What a game asks for is what a window somebody is looking at
            // does.
            assert_eq!(allowed(wanted, looking), wanted);
            // And a window they are not gives the pointer back, whatever the
            // game goes on asking for -- because it does go on asking: a game
            // has no way to know it lost focus, and its `cursor` is handed
            // nothing but its own view.
            assert_eq!(allowed(wanted, away), Cursor::Free);
            assert_eq!(allowed(wanted, hidden), Cursor::Free);
        }
    }

    #[test]
    fn a_delta_that_is_not_a_number_is_no_movement() {
        // A platform that reports a NaN delta -- which happens on a device that
        // was unplugged mid-motion -- would otherwise cast into something
        // arbitrary and jerk the camera.
        assert_eq!(round(f64::NAN), 0);
        assert_eq!(round(f64::INFINITY), 0);
        // A finite delta too large to count is a clamp rather than a zero,
        // because it is still a direction the player moved in.
        assert_eq!(round(1e30), i32::MAX);
        assert_eq!(round(-1e30), i32::MIN);
        assert_eq!(round(-3.7), -3);
        assert_eq!(round(3.7), 3);
    }
}
