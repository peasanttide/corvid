//! Building a point, taking it apart again, and the component order that ties
//! the two together.
//!
//! The order is `x`, `y`, `z`, and it is the same order in five places: the
//! constructors, `to_array`, `iter`, the digest, and the bytes on the wire. Any
//! one of those could be permuted on its own without the others noticing, and a
//! permutation is the kind of change that produces geometry which is wrong
//! rather than absent. So they are pinned to each other here, from a fixture
//! whose three components differ.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::explicit_iter_loop,
    reason = "`for c in point.iter()` is the spelling under test; rewriting it as `for c in &point` would leave the inherent method uncovered, which is the opposite of what this file is for"
)]

use corvid_fixed::{I16F16, I24F8, I48F16, Signed32};
use corvid_vector::{
    Direction, FinePoint, GlobalFinePoint, GlobalPoint, direction, finepoint, globalfinepoint,
    globalpoint,
};

#[test]
fn the_constructors_take_bare_integer_literals() {
    // The property the whole `From<integer>` design is for. An unsuffixed
    // literal reaching an `impl Into<Scalar>` parameter has to commit to one
    // integer type, and it can only do that while exactly one applies — so a
    // second lossless-but-unneeded impl on any of these scalars would turn every
    // line below into a compile error.
    assert_eq!(
        finepoint(1, 2, -3),
        FinePoint::new(
            I16F16::from_f64(1.0),
            I16F16::from_f64(2.0),
            I16F16::from_f64(-3.0)
        )
    );
    assert_eq!(
        globalpoint(1, 2, -3),
        GlobalPoint::new(
            I24F8::from_f64(1.0),
            I24F8::from_f64(2.0),
            I24F8::from_f64(-3.0)
        )
    );
    assert_eq!(
        globalfinepoint(1, 2, -3),
        GlobalFinePoint::new(
            I48F16::from_f64(1.0),
            I48F16::from_f64(2.0),
            I48F16::from_f64(-3.0)
        )
    );

    // And the widths those literals really reached, which is what says the
    // conversion placed them on the scale rather than somewhere reproducible.
    assert_eq!(globalfinepoint(1, 0, 0).x().to_bits(), 1 << 16);
    assert_eq!(globalpoint(1, 0, 0).x().to_bits(), 1 << 8);
    assert_eq!(finepoint(-1, 0, 0).x().to_bits(), -(1 << 16));
}

#[test]
fn a_constructor_takes_a_mix_of_scalars_and_integers() {
    // One type parameter per component rather than one shared by all three, so
    // the common shape — two axes at zero and one at a computed value — does not
    // force the caller to spell out the other two.
    let mixed = finepoint(I16F16::from_f64(0.5), 0, -1);
    assert_eq!(mixed.x(), I16F16::from_f64(0.5));
    assert_eq!(mixed.y(), I16F16::ZERO);
    assert_eq!(mixed.z(), I16F16::from_f64(-1.0));

    // `direction` has no integer conversion to reach for — no integer type has a
    // range inside `Signed32`'s `-1.0 ..= 1.0`, so there is nothing that could
    // be implemented exactly — and it takes the scalars themselves.
    let up = direction(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    assert_eq!(
        up,
        Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX)
    );
}

#[test]
fn tuples_and_arrays_convert_the_same_way_the_constructors_do() {
    let built = globalpoint(3, 4, 0);
    assert_eq!(GlobalPoint::from((3, 4, 0)), built);
    assert_eq!(GlobalPoint::from([3, 4, 0]), built);

    // A tuple's three components convert independently, like the constructor's.
    assert_eq!(
        GlobalPoint::from((I24F8::from_f64(3.0), 4, 0)),
        built,
        "a mixed tuple did not build the same point"
    );

    // And back out, which is what makes a point usable as a pattern.
    let (x, y, z) = <(I24F8, I24F8, I24F8)>::from(built);
    assert_eq!((x, y, z), (built.x(), built.y(), built.z()));
    assert_eq!(<[I24F8; 3]>::from(built), built.to_array());

    // The array conversion the generic impl replaced still works, which is the
    // thing a coherence-driven change like that has to preserve.
    let scalars = [I48F16::from_f64(1.0), I48F16::ZERO, I48F16::from_f64(-2.0)];
    assert_eq!(GlobalFinePoint::from(scalars).to_array(), scalars);
}

#[test]
fn the_component_order_is_the_same_everywhere() {
    // Three components that differ, so a permutation is visible. `1, 2, -3` also
    // makes a sign error visible, which a `1, 2, 3` fixture would not.
    let point = globalpoint(1, 2, -3);
    let expected = [
        I24F8::from_f64(1.0),
        I24F8::from_f64(2.0),
        I24F8::from_f64(-3.0),
    ];

    assert_eq!(point.to_array(), expected);
    assert_eq!(point.as_slice(), expected.as_slice());
    assert_eq!([point.x(), point.y(), point.z()], expected);
    assert_eq!(point.iter().copied().collect::<Vec<_>>(), expected.to_vec());
    assert_eq!(point.into_iter().collect::<Vec<_>>(), expected.to_vec());
    assert_eq!(
        (&point).into_iter().copied().collect::<Vec<_>>(),
        expected.to_vec()
    );

    // A permuted point is a different point, which is what makes the lines above
    // say something. Without this the fixture could be quietly flattened to
    // `splat` and every assertion would still pass.
    assert_ne!(globalpoint(-3, 2, 1), point);
}

#[test]
fn iter_mut_writes_through_to_the_components() {
    let mut point = finepoint(1, 2, 3);
    for component in point.iter_mut() {
        *component = component.saturating_mul(I16F16::from_f64(2.0));
    }
    assert_eq!(point, finepoint(2, 4, 6));

    // The by-reference `IntoIterator`, which is what a `for` loop over `&mut`
    // reaches for.
    for component in &mut point {
        *component = component.saturating_add(I16F16::from_f64(1.0));
    }
    assert_eq!(point, finepoint(3, 5, 7));

    // And a single component, in place. The order matters here too: writing
    // through index 2 has to be `z`.
    point.as_mut_slice()[2] = I16F16::ZERO;
    assert_eq!(point.z(), I16F16::ZERO);
    assert_eq!(point.x(), I16F16::from_f64(3.0));
}

#[test]
fn every_vector_type_iterates() {
    // The four types come out of one macro, so this is really a check that the
    // macro was not given the iteration for three of them and not the fourth.
    assert_eq!(globalfinepoint(1, 2, 3).into_iter().count(), 3);
    assert_eq!(globalpoint(1, 2, 3).into_iter().count(), 3);
    assert_eq!(finepoint(1, 2, 3).into_iter().count(), 3);
    assert_eq!(
        direction(Signed32::MAX, Signed32::ZERO, Signed32::ZERO)
            .into_iter()
            .count(),
        3
    );

    // A `Direction`'s components carry the SNORM denormal, and iterating hands
    // back what is stored rather than the canonical form — the same as
    // `to_array` and `x()`. It is `Eq` that folds, and this is what says
    // iteration did not quietly start folding too, which would make
    // `to_array` and `iter` disagree.
    let denormal = Direction::from_array([
        Signed32::from_bits(i32::MIN),
        Signed32::ZERO,
        Signed32::ZERO,
    ]);
    assert_eq!(
        denormal.iter().next().unwrap().to_bits(),
        i32::MIN,
        "iteration canonicalized a component that `to_array` does not"
    );
    assert_eq!(denormal.to_array()[0].to_bits(), i32::MIN);
}

#[test]
fn the_digest_absorbs_the_components_in_iteration_order() {
    use core::hash::Hash as _;

    use corvid_hash::{Hasher, digest};

    // The fifth place the order appears. A digest built by folding `iter` has to
    // equal the one the type produces, or `x, y, z` means two different things
    // depending on which side of the hash you are on.
    let point = globalpoint(1, 2, -3);
    let mut by_hand = Hasher::new();
    for component in point.iter() {
        component.hash(&mut by_hand);
    }
    assert_eq!(by_hand.digest(), digest(&point));

    // And the same fixture permuted digests differently, so the line above is
    // about order rather than about contents.
    assert_ne!(digest(&globalpoint(-3, 2, 1)), digest(&point));
}

#[cfg(feature = "serde")]
#[test]
fn the_wire_bytes_are_in_iteration_order() {
    // The sixth. `tests/wire.rs` freezes the bytes; this says the frozen bytes
    // are the components in the order `iter` yields them, which is the claim
    // that ties the golden to the accessors.
    let point = globalpoint(1, 2, -3);
    let written = corvid_wire::encode(&point).unwrap();
    let mut by_hand = Vec::new();
    for component in point.iter() {
        by_hand.extend_from_slice(&corvid_wire::encode(&component.to_bits()).unwrap());
    }
    assert_eq!(written, by_hand);
}
