//! A clip rectangle as the scissor a render pass takes.
//!
//! The seam is rounding: everything above works in `I16F16` layout units and a
//! scissor is whole pixels, so this is where the two meet and the one place
//! that answers `None` for a rectangle off the target.

use corvid_fixed::I16F16;
use corvid_render::Extent;
use corvid_ui::Rect;

/// A clip rectangle as the scissor a pass takes, or nothing when it is off the
/// target entirely.
///
/// A scissor outside the attachment is a validation error rather than an empty
/// draw, which is why this answers `None` rather than clamping to nothing.
#[must_use]
pub fn scissor(clip: Rect, viewport: Extent) -> Option<(u32, u32, u32, u32)> {
    // Widened before it is scaled and clamped once, which is
    // `corvid_fixed`'s job: a `u32` viewport past 32 767 px is outside what an
    // `I16F16` reaches, and shifting the narrow value first overflows rather
    // than saturating.
    let pixels = |edge: u32| I16F16::saturating_from_bits(i64::from(edge) << I16F16::FRAC_BITS);
    let whole = Rect::of(pixels(viewport.width), pixels(viewport.height));
    let inside = clip.intersection(whole);
    if inside.width.to_bits() <= 0 || inside.height.to_bits() <= 0 {
        return None;
    }
    Some((
        whole_pixels(inside.x),
        whole_pixels(inside.y),
        whole_pixels(inside.width),
        whole_pixels(inside.height),
    ))
}

/// A length as the whole pixels a scissor is in, rounded down.
const fn whole_pixels(value: I16F16) -> u32 {
    let bits = value.to_bits();
    if bits <= 0 {
        0
    } else {
        (bits >> 16).cast_unsigned()
    }
}
