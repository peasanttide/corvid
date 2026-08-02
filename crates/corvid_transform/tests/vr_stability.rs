//! What "does not visibly swim" means, stated as assertions rather than a vibe.
//!
//! Three properties, run against [`FineRotation`] as the eye pose **and**
//! against [`Rotation`] to document the difference. Property 3 is precisely
//! where the 32-bit tier is expected to fail, which is the evidence for having
//! two tiers at all.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "test helpers are only ever called at run time, and their arithmetic is written plainly so it stays readable as a reference"
)]
#![allow(
    clippy::print_stdout,
    reason = "the measured figures are the point; run with --nocapture to read them"
)]

mod common;

use corvid_transform::{
    Angle32, FineRotation, FineTransform, GlobalFinePoint, I48F16, Rotation, Versor,
};

/// A brisk but realistic head sweep.
const SWEEP_DEGREES_PER_SECOND: f64 = 200.0;

/// A headset's refresh rate.
const FRAME_HZ: f64 = 90.0;

/// 2.22 degrees per frame.
const STEP_DEGREES: f64 = SWEEP_DEGREES_PER_SECOND / FRAME_HZ;

/// `FineRotation`'s worst-case quantum, measured in `corvid_rotation`.
const FINE_QUANTUM_DEGREES: f64 = 0.0034;

/// `Rotation`'s worst-case quantum.
const COARSE_QUANTUM_DEGREES: f64 = 0.186;

// --- 1. Idempotence under a static pose ------------------------------------

#[test]
fn a_static_pose_decodes_identically_on_every_frame() {
    // Frame-to-frame dither is the artefact that reads as shimmer even when the
    // mean error is tiny. Integer arithmetic makes this exactly checkable
    // rather than statistical.
    let pose = common::pose(37.0, -12.0, 3.0);
    let first_basis = pose.to_basis();
    let first_versor = pose.to_versor();

    for frame in 0..10_000 {
        assert_eq!(
            pose.to_basis(),
            first_basis,
            "basis dithered on frame {frame}"
        );
        assert_eq!(
            pose.to_versor(),
            first_versor,
            "versor dithered on frame {frame}"
        );
    }
}

#[test]
fn a_static_camera_projects_a_static_point_every_frame() {
    // The whole pipeline, not just the codec: a fixed camera and a fixed world
    // point must give bit-identical eye coordinates every frame.
    let camera = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_f64(6_371_000.0)),
        common::pose(37.0, -12.0, 3.0),
    );
    let target = camera.position()
        + GlobalFinePoint::new(
            I48F16::from_f64(1.5),
            I48F16::from_f64(-0.25),
            I48F16::from_f64(0.75),
        );

    let first = camera.to_fine_global(target).expect("near field");
    for frame in 0..10_000 {
        assert_eq!(
            camera.to_fine_global(target),
            Some(first),
            "the eye-space position moved on frame {frame}"
        );
    }
}

// --- 2. No dither near a quantization boundary -----------------------------

#[test]
fn a_pose_at_a_cell_edge_does_not_oscillate_between_cells() {
    // Sit a pose deliberately at a quantization boundary and perturb it by less
    // than half a quantum. This is where a codec betrays you: a naive one flips
    // between two cells and the image shimmers even though the head is still.
    //
    // The boundary is found rather than assumed: sweep in tiny steps until the
    // encoding changes, which *is* a cell edge by definition.
    let mut edge_degrees = 0.0f64;
    let mut previous = FineRotation::from_versor(yaw(0.0)).to_bits();
    let mut found = false;
    for step in 1..20_000 {
        let degrees = f64::from(step) * 1e-5;
        let bits = FineRotation::from_versor(yaw(degrees)).to_bits();
        if bits != previous {
            edge_degrees = degrees;
            found = true;
            break;
        }
        previous = bits;
    }
    assert!(found, "no quantization boundary found in the swept range");

    // Now jitter around that edge by well under half a quantum and require the
    // decode to be constant. Sitting exactly *on* the edge is a coin flip by
    // construction, so start a fifth of a quantum inside the cell.
    let inside = edge_degrees + FINE_QUANTUM_DEGREES * 0.2;
    let jitter = FINE_QUANTUM_DEGREES * 0.1;

    let reference = FineRotation::from_versor(yaw(inside)).to_bits();
    for step in 0..1_000 {
        let offset = (f64::from(step % 21) - 10.0) / 10.0 * jitter;
        let bits = FineRotation::from_versor(yaw(inside + offset)).to_bits();
        assert_eq!(
            bits, reference,
            "cell dither at step {step}: {offset} degrees of jitter changed the encoding"
        );
    }
    println!("FineRotation cell edge at {edge_degrees:.5} deg; no dither within +-{jitter:.5} deg");
}

// --- 3. Bounded step under a monotone sweep --------------------------------

#[test]
fn a_monotone_sweep_has_bounded_steps_at_the_fine_tier() {
    // Quantization must never manifest as a visible stutter or reversal in an
    // otherwise smooth motion.
    let (worst, reversed) = sweep_deviation(|q| FineRotation::from_versor(q).to_versor());
    println!("FineRotation sweep: worst step deviation {worst:.5} deg, {reversed} reversals");

    assert!(
        worst <= 2.0 * FINE_QUANTUM_DEGREES,
        "step deviation {worst} degrees exceeds 2 quanta ({})",
        2.0 * FINE_QUANTUM_DEGREES
    );
    assert_eq!(reversed, 0, "the sweep reversed direction");
}

#[test]
fn the_same_sweep_at_the_coarse_tier_shows_why_there_are_two_tiers() {
    // Rotation's quantum is 0.186 degrees against a 2.22 degree step, so the
    // steps visibly vary. Documented here rather than asserted away.
    let (worst, _) = sweep_deviation(|q| Rotation::from_versor(q).to_versor());
    println!(
        "Rotation sweep:     worst step deviation {worst:.5} deg \
         (FineRotation holds {:.5})",
        2.0 * FINE_QUANTUM_DEGREES
    );

    // The coarse tier misses the bound the fine tier holds, by a wide margin —
    // which is the evidence for carrying two tiers.
    assert!(
        worst > 2.0 * FINE_QUANTUM_DEGREES,
        "the coarse tier unexpectedly met the fine tier's bound"
    );
    // But it is still bounded by its own quantum, so it is usable for objects.
    assert!(
        worst <= 2.0 * COARSE_QUANTUM_DEGREES,
        "coarse deviation {worst} exceeds even its own two quanta"
    );
}

/// The **signed** yaw carried by the rotation taking `from` to `to`.
///
/// Positive when the head turned the way the sweep is going. `Versor::angle_to`
/// cannot answer this: it is an unsigned magnitude.
fn signed_yaw_step(from: Versor, to: Versor) -> f64 {
    let relative = from.inverse().compose(to);
    // For a yaw about +Z the relative versor is `(0, 0, sin(θ/2), cos(θ/2))`,
    // so `z · w` carries the sign of θ over a half turn either way.
    let q = relative.to_xyzw();
    let signed = q[2].to_f64() * q[3].to_f64();
    if signed >= 0.0 {
        relative.angle_to(Versor::IDENTITY).to_degrees()
    } else {
        -1.0
    }
}

/// A yaw-only pose, as a versor.
const fn yaw(degrees: f64) -> Versor {
    Versor::from_yaw_pitch_roll(
        Angle32::from_degrees(degrees),
        corvid_transform::Pitch32::ZERO,
        Angle32::ZERO,
    )
}

/// Sweeps the head at 200°/s sampled at 90 Hz through `codec`, and returns the
/// worst deviation of a frame-to-frame step from the ideal 2.22°, along with
/// the number of frames where the motion reversed.
fn sweep_deviation(mut codec: impl FnMut(Versor) -> Versor) -> (f64, u32) {
    let mut worst = 0.0f64;
    let mut reversed = 0;
    let mut previous: Option<Versor> = None;

    for frame in 0..900 {
        let decoded = codec(yaw(f64::from(frame) * STEP_DEGREES));
        if let Some(prev) = previous {
            let step = prev.angle_to(decoded).to_degrees();
            worst = worst.max((step - STEP_DEGREES).abs());
            // `angle_to` is `2·acos(|dot|)` and so never negative — testing it
            // against zero could not detect a reversal. The sweep is a yaw, so
            // the sign of the step is the sign of the relative rotation's `z`
            // component, which is what actually says which way the head turned.
            if signed_yaw_step(prev, decoded) <= 0.0 {
                reversed += 1;
            }
        }
        previous = Some(decoded);
    }
    (worst, reversed)
}
