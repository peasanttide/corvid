//! A rumour spreading along a strip, and nothing of it lost.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

#[allow(
    dead_code,
    unreachable_pub,
    reason = "one fixture module serves every test file, and each file uses the surfaces it needs"
)]
mod surface;

use corvid_fixed::{Factor16, I16F16};
use corvid_nav::{NavError, NavTriRef, diffuse_step};

use surface::strip;

/// The sum of a field, as the integers it is made of.
///
/// Summed as bit patterns rather than as values, because what conservation
/// means here is that not one last bit went anywhere: a total compared as `f64`
/// would call a step that lost a few thousand of them equal.
fn total(field: &[I16F16]) -> i64 {
    field.iter().map(|value| i64::from(value.to_bits())).sum()
}

/// One triangle's worth of rumour, spread along eight triangles, is still one
/// triangle's worth.
#[test]
fn a_diffusion_step_conserves_the_total() {
    let mesh = strip(4);
    assert_eq!(mesh.len(), 8);

    let mut field = vec![I16F16::ZERO; mesh.len()];
    if let Some(slot) = field.first_mut() {
        *slot = I16F16::ONE;
    }
    let before = total(&field);

    for _ in 0..64 {
        diffuse_step(&mesh, &mut field, Factor16::from_f64(0.5))
            .expect("a field of the right size");
        assert_eq!(
            total(&field),
            before,
            "every step moves the same integer out of one triangle and into another"
        );
    }
}

/// It spreads, and it spreads along the strip rather than jumping down it.
///
/// After one step only the first triangle's neighbours have heard anything;
/// after sixty-four the far end has, and the near end has less than it started
/// with.
#[test]
fn a_rumour_travels_along_the_seams() {
    let mesh = strip(4);
    let mut field = vec![I16F16::ZERO; mesh.len()];
    if let Some(slot) = field.first_mut() {
        *slot = I16F16::ONE;
    }

    diffuse_step(&mesh, &mut field, Factor16::from_f64(0.5)).expect("a step");
    assert!(field[1] > I16F16::ZERO, "the neighbour has heard");
    assert_eq!(
        field[7],
        I16F16::ZERO,
        "and the far end, four squares away, has not"
    );

    for _ in 0..63 {
        diffuse_step(&mesh, &mut field, Factor16::from_f64(0.5)).expect("a step");
    }
    assert!(field[7] > I16F16::ZERO, "by now it has got about");
    assert!(
        field[0] < I16F16::ONE,
        "and the first triangle has let go of some"
    );
}

/// A field that is not the mesh's length is the one way an index-parallel array
/// can be wrong, and it is refused rather than truncated.
#[test]
fn a_field_of_the_wrong_length_is_refused() {
    let mesh = strip(2);
    let mut field = vec![I16F16::ZERO; 3];
    assert_eq!(
        diffuse_step(&mesh, &mut field, Factor16::MAX),
        Err(NavError::FieldLengthMismatch { field: 3, tris: 4 })
    );
}

/// Diffusion visits seams in triangle order and then edge order, so the same
/// field diffuses to the same bits every time.
#[test]
fn a_diffusion_step_is_reproducible() {
    let mesh = strip(3);
    let seed: Vec<I16F16> = (0..mesh.len())
        .map(|index| I16F16::from_bits((i32::try_from(index).unwrap_or(0) + 1) * 7919))
        .collect();

    let mut once = seed.clone();
    let mut twice = seed;
    diffuse_step(&mesh, &mut once, Factor16::from_f64(0.25)).expect("a step");
    diffuse_step(&mesh, &mut twice, Factor16::from_f64(0.25)).expect("a step");
    assert_eq!(once, twice);

    // And the neighbour iteration it reads is the same order every time too.
    let neighbours: Vec<NavTriRef> = mesh.neighbours(NavTriRef(2)).collect();
    assert_eq!(
        neighbours,
        mesh.neighbours(NavTriRef(2)).collect::<Vec<_>>()
    );
}
