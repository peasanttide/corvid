//! `Basis` and `Versor`: orthonormality, round trips, and composition order.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use common::Rng;
use corvid_fixed::{Angle32, Factor32, I2F30, I16F16, I48F16, Pitch32, Signed32};
use corvid_rotation::{Basis, Versor};
use corvid_vector::{Direction, FinePoint, GlobalFinePoint};

/// A few hundred last bits of `I2F30`, which is what a rotation that has been
/// through the codecs and back can drift by.
const CLOSE: I2F30 = I2F30::from_bits(1 << 12);

#[test]
fn a_basis_is_orthonormal_with_unit_determinant() {
    let mut rng = Rng::new(0x0A17_0011);
    for _ in 0..20_000 {
        let m = common::random_basis(&mut rng);
        // M . M^T = I.
        assert!(
            m.compose(m.inverse()).abs_diff_eq(Basis::IDENTITY, CLOSE),
            "M M^T is not the identity: {:?}",
            m.compose(m.inverse())
        );
        assert!(m.inverse().compose(m).abs_diff_eq(Basis::IDENTITY, CLOSE));
        // And the crate's own checker agrees.
        assert!(Basis::from_rows(m.to_rows()).is_some());
    }
}

#[test]
fn identity_faces_positive_y_with_positive_z_up() {
    assert_eq!(
        Basis::IDENTITY.forward(),
        Direction::new(Signed32::ZERO, Signed32::MAX, Signed32::ZERO)
    );
    assert_eq!(
        Basis::IDENTITY.up(),
        Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX)
    );
    assert_eq!(
        Basis::IDENTITY.right(),
        Direction::new(Signed32::MAX, Signed32::ZERO, Signed32::ZERO)
    );
    assert_eq!(Basis::default(), Basis::IDENTITY);
    assert_eq!(Versor::default(), Versor::IDENTITY);
}

#[test]
fn the_identity_leaves_every_point_alone() {
    let mut rng = Rng::new(0x1DE_1DE);
    for _ in 0..20_000 {
        let v = common::random_fine_point(&mut rng, 30_000.0);
        assert_eq!(Basis::IDENTITY.rotate_fine(v), v);
        assert_eq!(Basis::IDENTITY.unrotate_fine(v), v);
        assert_eq!(Versor::IDENTITY.rotate_fine(v), v);
    }
}

#[test]
fn a_rotation_rounds_a_tie_away_from_zero_at_both_widths() {
    // `round_shift_i64` and `round_shift_i128` in `src/basis.rs` round half
    // away from zero, the rule `quantize` and the rest of the crate's
    // reductions use, so a rotation and its mirror image land on components
    // that are exact negations rather than one step apart. Nothing else here
    // can see that: only an exact tie separates it from rounding half up or
    // half down, and no random pose produces one.
    //
    // This basis is a sixth of a turn in the right-forward plane, whose cosine
    // is exactly one half — `1 << 29` at Q30 — so `row[0] · (x, 0, 0)` is
    // `x / 2` exactly and a component of one raw unit is exactly half a step.
    // The sine is irrational and lands wherever Q30 puts it; the rows are still
    // orthonormal to well within `from_rows`'s tolerance, which is what makes
    // this a basis the public constructor accepts rather than one smuggled in.
    const HALF: I2F30 = I2F30::from_bits(1 << 29);
    const SIN: I2F30 = I2F30::from_bits(929_887_697);
    let m = Basis::from_rows([
        [HALF, I2F30::from_bits(-SIN.to_bits()), I2F30::ZERO],
        [SIN, HALF, I2F30::ZERO],
        [I2F30::ZERO, I2F30::ZERO, I2F30::ONE],
    ])
    .expect("a sixth of a turn is a rotation");

    // The `i64` path, through `I16F16` components.
    let fine = |bits: i32| {
        m.rotate_fine(FinePoint::new(
            I16F16::from_bits(bits),
            I16F16::ZERO,
            I16F16::ZERO,
        ))
        .to_array()[0]
            .to_bits()
    };
    assert_eq!(fine(1), 1, "a positive tie rounded toward zero");
    assert_eq!(fine(-1), -1, "a negative tie rounded toward zero");

    // The `i128` path, through `I48F16` components, which is a second
    // implementation of the same rule and was reached by nothing above.
    let wide = |bits: i64| {
        m.rotate_global_fine(GlobalFinePoint::new(
            I48F16::from_bits(bits),
            I48F16::ZERO,
            I48F16::ZERO,
        ))
        .to_array()[0]
            .to_bits()
    };
    assert_eq!(wide(1), 1, "a positive tie rounded toward zero at i128");
    assert_eq!(wide(-1), -1, "a negative tie rounded toward zero at i128");

    // Stated as the symmetry it exists for, rather than only as two constants:
    // negating the input negates the output exactly, which is what "half away
    // from zero" buys and what half up or half down would break on this pair.
    assert_eq!(fine(1), -fine(-1));
    assert_eq!(wide(1), -wide(-1));
}

#[test]
fn untransform_undoes_transform() {
    let mut rng = Rng::new(0x1111_7777);
    for _ in 0..20_000 {
        let m = common::random_basis(&mut rng);
        let v = common::random_fine_point(&mut rng, 1000.0);
        let round_tripped = m.unrotate_fine(m.rotate_fine(v));
        // Two roundings at 15.26 um each, plus the basis's own quantization
        // scaled by the point's magnitude.
        assert!(
            common::within(round_tripped, v, I16F16::from_bits(8)),
            "{v:?} round-tripped to {round_tripped:?}"
        );
    }
}

#[test]
fn rotation_preserves_length() {
    let mut rng = Rng::new(0x1E_9714);
    for _ in 0..20_000 {
        let m = common::random_basis(&mut rng);
        let v = common::random_fine_point(&mut rng, 1000.0);
        let rotated = m.rotate_fine(v);
        let before = v.length().to_f64();
        let after = rotated.length().to_f64();
        assert!(
            (before - after).abs() < 0.01,
            "length {before} became {after}"
        );
    }
}

#[test]
fn composition_is_associative() {
    let mut rng = Rng::new(0x0A55_0C11);
    for _ in 0..5_000 {
        let a = common::random_basis(&mut rng);
        let b = common::random_basis(&mut rng);
        let c = common::random_basis(&mut rng);
        assert!(
            a.compose(b)
                .compose(c)
                .abs_diff_eq(a.compose(b.compose(c)), CLOSE),
            "composition is not associative"
        );
    }
}

#[test]
fn compose_applies_the_right_hand_operand_first() {
    // a.compose(b) applies b first, then a — matrix multiplication order and
    // glam's Mul. This test fails if the order is ever flipped.
    let yaw = Basis::from_yaw_pitch_roll(Angle32::from_degrees(90.0), Pitch32::ZERO, Angle32::ZERO);
    let pitch =
        Basis::from_yaw_pitch_roll(Angle32::ZERO, Pitch32::from_degrees(90.0), Angle32::ZERO);

    let mut rng = Rng::new(0x0DE2_0077);
    for _ in 0..1_000 {
        let v = common::random_fine_point(&mut rng, 1000.0);
        assert!(
            common::within(
                yaw.compose(pitch).rotate_fine(v),
                yaw.rotate_fine(pitch.rotate_fine(v)),
                I16F16::from_bits(16),
            ),
            "compose applied its operands in the wrong order"
        );
        // And the other order is genuinely different, so the test has teeth.
        assert!(
            !common::within(
                yaw.compose(pitch).rotate_fine(v),
                pitch.rotate_fine(yaw.rotate_fine(v)),
                I16F16::from_bits(16),
            ) || v.is_zero()
        );
    }
}

#[test]
fn inverse_is_the_transpose_and_undoes_composition() {
    let mut rng = Rng::new(0x1_1E45E);
    for _ in 0..5_000 {
        let a = common::random_basis(&mut rng);
        let b = common::random_basis(&mut rng);
        // (a . b)^-1 = b^-1 . a^-1
        assert!(
            a.compose(b)
                .inverse()
                .abs_diff_eq(b.inverse().compose(a.inverse()), CLOSE)
        );
        // The transpose, spelled out.
        let rows = a.to_rows();
        let transposed = a.inverse().to_rows();
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(rows[i][j], transposed[j][i]);
            }
        }
    }
}

// --- Versor ----------------------------------------------------------------

#[test]
fn basis_and_versor_agree_on_every_rotation() {
    let mut rng = Rng::new(0xE250_0031);
    for _ in 0..20_000 {
        let q = common::random_versor(&mut rng);
        let m = q.to_basis();
        let v = common::random_fine_point(&mut rng, 1000.0);
        assert!(
            common::within(q.rotate_fine(v), m.rotate_fine(v), I16F16::from_bits(8)),
            "Versor and Basis disagree"
        );
        assert!(common::direction_within(q.forward(), m.forward(), 1e-6));
    }
}

#[test]
fn versor_and_basis_round_trip_through_each_other() {
    let mut rng = Rng::new(0x9007_9007);
    for _ in 0..20_000 {
        let q = common::random_versor(&mut rng);
        let back = Versor::from_basis(q.to_basis());
        // Compared component-wise rather than through `angle_to`: `acos` is
        // ill-conditioned at 1, so it reports ~0.0025 degrees for rotations
        // that are in fact a last bit apart. Up to sign, because the double
        // cover means -q is the same rotation.
        let aligned = if q.dot(back).is_negative() {
            back.negate()
        } else {
            back
        };
        assert!(
            q.abs_diff_eq(aligned, I2F30::from_bits(1 << 8)),
            "{q:?} became {back:?}"
        );
    }
}

#[test]
fn versor_composition_matches_matrix_composition() {
    let mut rng = Rng::new(0xC0AB_0051);
    for _ in 0..5_000 {
        let a = common::random_versor(&mut rng);
        let b = common::random_versor(&mut rng);
        let composed = a.compose(b);
        let via_matrix = Versor::from_basis(a.to_basis().compose(b.to_basis()));
        assert!(
            composed.angle_to(via_matrix).to_degrees() < 0.01,
            "{} degrees apart",
            composed.angle_to(via_matrix).to_degrees()
        );
    }
}

#[test]
fn repeated_composition_drifts_slowly_and_renormalize_fixes_it() {
    // `compose` deliberately does not renormalize — folding an `rsqrt` into it
    // would cost an order of magnitude and hand the win back to the matrix. The
    // price is a slow drift off the unit sphere, and this test states its size.
    let mut rng = Rng::new(0xD21F_0013);
    let step = common::random_versor(&mut rng);

    let mut drifting = Versor::IDENTITY;
    let mut corrected = Versor::IDENTITY;
    for _ in 0..1_000 {
        drifting = drifting.compose(step);
        corrected = corrected.compose(step).renormalize();
    }

    let squared_norm = |q: Versor| {
        let [x, y, z, w] = common::to_f64_quaternion(q);
        x * x + y * y + z * z + w * w
    };

    // A thousand composes stay well inside a part in a thousand, which is far
    // below what any rotation consumer notices...
    let drift = (squared_norm(drifting) - 1.0).abs();
    assert!(
        drift < 1e-3,
        "a thousand composes drifted to squared norm {drift}"
    );
    // ...and renormalizing pins it to the sphere outright.
    assert!((squared_norm(corrected) - 1.0).abs() < 1e-8);

    // Either way the rotation itself is still sound.
    assert!(Basis::from_rows(corrected.to_basis().to_rows()).is_some());
}

#[test]
fn the_conjugate_is_the_inverse() {
    let mut rng = Rng::new(0xC0A1_0019);
    for _ in 0..20_000 {
        let q = common::random_versor(&mut rng);
        assert_eq!(q.inverse(), q.conjugate());
        assert!(
            q.compose(q.inverse())
                .abs_diff_eq(Versor::IDENTITY, I2F30::from_bits(1 << 8))
        );
    }
}

#[test]
fn negating_a_versor_names_the_same_rotation() {
    let mut rng = Rng::new(0x0E60_0023);
    for _ in 0..20_000 {
        let q = common::random_versor(&mut rng);
        // The matrix form is the sign-free statement of "same rotation", and
        // it is exact — unlike `angle_to`, whose `acos` cannot resolve zero.
        assert_eq!(q.to_basis(), q.negate().to_basis());
        assert_eq!(q.negate().negate(), q);
        assert!(q.angle_to(q.negate()).to_degrees() < 0.01);
    }
}

#[test]
fn from_xyzw_rejects_anything_that_is_not_unit() {
    let one = I2F30::ONE;
    let zero = I2F30::ZERO;
    assert!(Versor::from_xyzw(zero, zero, zero, one).is_some());
    assert_eq!(Versor::from_xyzw(one, one, one, one), None);
    assert_eq!(Versor::from_xyzw(zero, zero, zero, zero), None);
    assert_eq!(
        Versor::from_xyzw(I2F30::from_f64(0.5), zero, zero, zero),
        None
    );
}

#[test]
fn slerp_has_constant_angular_velocity() {
    let axis = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    let a = Versor::IDENTITY;
    let b = Versor::from_axis_angle(axis, Angle32::from_degrees(90.0));

    assert_eq!(a.slerp(b, Factor32::ZERO), a);
    assert!(a.slerp(b, Factor32::ONE).angle_to(b).to_degrees() < 0.01);

    // Equal parameter steps give equal angular steps.
    for i in 0..8u32 {
        let t0 = Factor32::from_f64(f64::from(i) / 8.0);
        let t1 = Factor32::from_f64(f64::from(i + 1) / 8.0);
        let step = a.slerp(b, t0).angle_to(a.slerp(b, t1)).to_degrees();
        assert!((step - 90.0 / 8.0).abs() < 0.05, "step {i}: {step} degrees");
    }
}

#[test]
fn nlerp_tracks_slerp_over_the_angles_a_frame_actually_spans() {
    let axis = Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
    let a = Versor::IDENTITY;

    // Three degrees is a brisk head turn at 90 Hz. Over that span the two are
    // indistinguishable, which is why nlerp is the default.
    let small = Versor::from_axis_angle(axis, Angle32::from_degrees(3.0));
    for i in 0..=8u32 {
        let t = Factor32::from_f64(f64::from(i) / 8.0);
        let apart = a.nlerp(small, t).angle_to(a.slerp(small, t)).to_degrees();
        assert!(
            apart < 0.01,
            "nlerp and slerp {apart} degrees apart at t = {i}/8"
        );
    }

    // Over 90 degrees they visibly diverge, which is the trade being made.
    let quarter = Versor::from_axis_angle(axis, Angle32::from_degrees(90.0));
    let half = Factor32::from_f64(0.5);
    assert!(
        a.nlerp(quarter, half)
            .angle_to(a.slerp(quarter, half))
            .to_degrees()
            < 1.0
    );

    // Both are exact at the ends.
    assert_eq!(a.nlerp(quarter, Factor32::ZERO), a);
    assert!(
        a.nlerp(quarter, Factor32::ONE)
            .angle_to(quarter)
            .to_degrees()
            < 0.01
    );
}

#[test]
fn the_working_types_are_available_in_const_context() {
    const M: Basis = Basis::IDENTITY;
    const Q: Versor = Versor::IDENTITY;
    const COMPOSED: Basis = M.compose(M);
    const INVERTED: Basis = M.inverse();
    const AS_VERSOR: Versor = M.to_versor_const();
    const AS_BASIS: Basis = Q.to_basis();
    const ROTATED: FinePoint = M.rotate_fine(FinePoint::ZERO);

    assert_eq!(COMPOSED, Basis::IDENTITY);
    assert_eq!(INVERTED, Basis::IDENTITY);
    assert_eq!(AS_VERSOR, Versor::IDENTITY);
    assert_eq!(AS_BASIS, Basis::IDENTITY);
    assert_eq!(ROTATED, FinePoint::ZERO);
}
