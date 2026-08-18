//! Dragging a local coordinate back onto the triangle it is supposed to be on.
//!
//! Two clamps, and the difference between them is the whole of the seam bug:
//! one leaves a body exactly on the edge it just crossed and the other leaves
//! it a margin inside.

use corvid_fixed::I16F16;
use corvid_vector::FinePoint;

/// How far inside a triangle an event leaves a body that landed on its edge.
///
/// Two position codes, which is one part in 32768 of the triangle: 61 um across
/// a two-metre face and 12 cm across one at [`MAX_EDGE`](crate::MAX_EDGE).
/// What it buys is that
/// no boundary is ever at distance zero from a body that has just finished
/// crossing it, which is what stopped a walker dead on a seam.
pub const EDGE_MARGIN: I16F16 = I16F16::from_bits(2);

/// The nearest local coordinate inside the triangle, and not underground.
#[must_use]
#[inline]
pub(crate) const fn clamp(local: FinePoint) -> FinePoint {
    let mut x = at_least_zero(local.x().to_bits()) as i64;
    let mut y = at_least_zero(local.y().to_bits()) as i64;
    let one = I16F16::ONE.to_bits() as i64;
    let over = x + y - one;
    if over > 0 {
        // Both weights give up the same share, so a point outside a corner
        // lands on the edge nearest it rather than on whichever axis the code
        // happened to test first.
        let half = over / 2;
        x -= half;
        y -= over - half;
        if y < 0 {
            x += y;
            y = 0;
        }
        if x < 0 {
            y += x;
            x = 0;
        }
    }
    FinePoint::new(
        I16F16::from_bits(x as i32),
        I16F16::from_bits(y as i32),
        I16F16::from_bits(at_least_zero(local.z().to_bits())),
    )
}

/// The nearest local coordinate inside the triangle by [`EDGE_MARGIN`], and not
/// underground.
#[must_use]
#[inline]
pub(crate) const fn settle(local: FinePoint) -> FinePoint {
    let margin = EDGE_MARGIN.to_bits() as i64;
    let one = I16F16::ONE.to_bits() as i64;
    let inside = clamp(local);
    let mut x = inside.x().to_bits() as i64;
    let mut y = inside.y().to_bits() as i64;
    if x < margin {
        x = margin;
    }
    if y < margin {
        y = margin;
    }
    let over = x + y - (one - margin);
    if over > 0 {
        // Both weights give up the same share, so a body settling out of a
        // corner leaves along the bisector rather than along whichever axis the
        // code tested first.
        let half = over / 2;
        x -= half;
        y -= over - half;
        if x < margin {
            x = margin;
        }
        if y < margin {
            y = margin;
        }
    }
    FinePoint::new(
        I16F16::from_bits(x as i32),
        I16F16::from_bits(y as i32),
        inside.z(),
    )
}

/// `value`, or zero if it is below.
#[inline]
const fn at_least_zero(value: i32) -> i32 {
    if value < 0 { 0 } else { value }
}
