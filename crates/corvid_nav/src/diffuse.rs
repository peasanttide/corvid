//! Spreading a per-triangle quantity across the surface.

use corvid_fixed::{Factor16, I16F16};

use crate::error::NavError;
use crate::mesh::NavMesh;
use crate::seam::NavTriEdge;

/// Spreads a per-triangle field one step across its seams.
///
/// Written as **flows on edges** rather than as an average of neighbours, and
/// that is the whole of why it conserves: each seam is visited once, from the
/// lower-indexed side, and whatever it takes from one triangle it gives to the
/// other in the same integer. There is no rounding to lose, because the
/// rounding happens once, to the flow, and the flow is what both sides see.
///
/// Each seam moves `rate / 3` of the difference across it, so a triangle with
/// three neighbours sheds at most `rate` of its excess in a step and the field
/// cannot overshoot. Seams are taken in triangle order and then edge order,
/// which is what makes the answer a fact about the mesh rather than about the
/// iteration; the field is updated in place as it goes, so a triangle already
/// visited diffuses with its new value rather than its old one. That is a
/// choice, not an oversight: it halves the memory a million-triangle field
/// costs and it conserves either way.
///
/// Walkability is not consulted. A rumour crosses a wall a peasant cannot.
///
/// # Errors
///
/// [`NavError::FieldLengthMismatch`] if the field is not exactly as long as the
/// mesh, which is the one way an index-parallel array can be wrong.
pub fn diffuse_step(mesh: &NavMesh, field: &mut [I16F16], rate: Factor16) -> Result<(), NavError> {
    if field.len() != mesh.len() {
        return Err(NavError::FieldLengthMismatch {
            field: field.len(),
            tris: mesh.len(),
        });
    }

    for (index, tri) in mesh.tris().iter().enumerate() {
        for seam in tri.edges().into_iter().flatten().map(NavTriEdge::next) {
            let other = seam.0 as usize;
            if other <= index {
                continue;
            }
            let (Some(&here), Some(&there)) = (field.get(index), field.get(other)) else {
                continue;
            };
            let flow = share(here.saturating_sub(there), rate);
            if let Some(slot) = field.get_mut(index) {
                *slot = slot.saturating_sub(flow);
            }
            if let Some(slot) = field.get_mut(other) {
                *slot = slot.saturating_add(flow);
            }
        }
    }
    Ok(())
}

/// One seam's share of a difference, truncated toward zero.
///
/// Truncating rather than rounding, and toward zero rather than down, so that
/// reversing the two sides reverses the flow exactly: a rounding that leaned
/// one way would make the field drift in whichever direction the indices
/// happened to run.
#[inline]
fn share(difference: I16F16, rate: Factor16) -> I16F16 {
    let scaled = i64::from(difference.to_bits()) * i64::from(rate.to_bits());
    I16F16::from_bits((scaled / (65_535 * 3)) as i32)
}
