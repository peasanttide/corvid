//! The fast trigonometry, and the bounds it trades accuracy for.
//!
//! Nothing here is correctly rounded and nothing claims to be. What is checked
//! is the documented error, the symmetries that survive the approximation, and
//! that the exactness at the quarter turns is not one of the things given up.

#![allow(
    clippy::panic_in_result_fn,
    clippy::missing_panics_doc,
    clippy::float_cmp,
    reason = "tests assert; a panic is how a test reports failure"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    reason = "these tests feed edge-case bit patterns through narrowing casts on purpose"
)]
mod common;

use common::{Rng, Worst};
use corvid_fixed::{Angle8, Angle16, Angle32, Signed8, Signed16};

/// The reference sine of a phase, as the output type's nearest bit pattern.
fn reference(phase: f64, turn: f64, scale: f64, quarter_offset: f64) -> i128 {
    let radians = (phase / turn + quarter_offset) * core::f64::consts::TAU;
    (radians.sin() * scale).round() as i128
}
#[test]
fn fast_sine_is_within_its_documented_error() {
    let scale = f64::from(Signed16::MAX.to_bits());
    let mut worst = Worst::default();
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        worst.observe(
            i128::from(bits),
            i128::from(angle.sin_fast().to_bits()),
            reference(f64::from(bits), 65_536.0, scale, 0.0),
        );
    }
    // 1.2e-3 of full scale, in units of Signed16's last bit. The worst over this
    // domain is 1.0965e-3, at bits 63483; over the full 2^32 phases it is
    // 1.1111e-3, which the exhaustive test below pins. That is why the documented
    // bound is 1.2e-3 and not the 1.1e-3 the 16-bit sweep alone would suggest.
    let limit = (1.2e-3 * scale).ceil() as i128;
    worst.assert_within(limit, "Angle16::sin_fast");

    // The same bound stated the way the docs state it, so a doc that drifts
    // away from the implementation fails here rather than misleading a caller
    // budgeting error.
    let mut worst_absolute = 0.0_f64;
    for bits in 0..=u16::MAX {
        let expected = (f64::from(bits) / 65_536.0 * core::f64::consts::TAU).sin();
        let actual = Angle16::from_bits(bits).sin_fast().to_f64();
        worst_absolute = worst_absolute.max((actual - expected).abs());
    }
    assert!(
        worst_absolute <= 1.2e-3,
        "sin_fast worst absolute error {worst_absolute:e} exceeds the documented 1.2e-3"
    );
    assert!(
        worst_absolute > 1.05e-3,
        "sin_fast improved to {worst_absolute:e}; tighten the documented bound"
    );

    // At 8-bit output the approximation is already exact to the last bit.
    let scale8 = f64::from(Signed8::MAX.to_bits());
    let mut worst8 = Worst::default();
    for bits in 0..=u8::MAX {
        worst8.observe(
            i128::from(bits),
            i128::from(Angle8::from_bits(bits).sin_fast().to_bits()),
            reference(f64::from(bits), 256.0, scale8, 0.0),
        );
    }
    worst8.assert_within(1, "Angle8::sin_fast");
}

#[test]
fn fast_trigonometry_is_exact_at_the_quarter_turns() {
    assert_eq!(Angle16::ZERO.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::QUARTER_TURN.sin_fast(), Signed16::MAX);
    assert_eq!(Angle16::HALF_TURN.sin_fast(), Signed16::ZERO);
    assert_eq!(Angle16::THREE_QUARTER_TURN.sin_fast(), Signed16::MIN);

    assert_eq!(Angle16::ZERO.cos_fast(), Signed16::MAX);
    assert_eq!(Angle16::QUARTER_TURN.cos_fast(), Signed16::ZERO);
    assert_eq!(Angle16::HALF_TURN.cos_fast(), Signed16::MIN);
}

#[test]
fn fast_sine_is_exactly_odd_and_fast_cosine_exactly_even() {
    // Not "within a bit of odd" -- exactly odd, because the phase is folded about
    // the peak rather than shifted one-sidedly. Worth pinning: the property is
    // free from that fold and would be silently lost by a refactor that reached
    // for the cheaper-looking arithmetic.
    for bits in 0..=u16::MAX {
        let angle = Angle16::from_bits(bits);
        let mirror = Angle16::from_bits(bits.wrapping_neg());
        assert_eq!(
            mirror.sin_fast().to_bits(),
            -angle.sin_fast().to_bits(),
            "sin_fast is not odd at {bits}"
        );
        assert_eq!(
            mirror.cos_fast().to_bits(),
            angle.cos_fast().to_bits(),
            "cos_fast is not even at {bits}"
        );
    }

    // Negating the result above is only sound because the scale never reaches
    // the denormal bit pattern that a signed-normalized type reserves.
    for bits in 0..=u16::MAX {
        assert_ne!(
            Angle16::from_bits(bits).sin_fast().to_bits(),
            i16::MIN,
            "sin_fast emitted a denormal at {bits}"
        );
    }
}

#[test]
fn fast_atan2_is_exact_on_the_axes() {
    assert_eq!(Angle16::atan2_fast(0, 1), Angle16::ZERO);
    assert_eq!(Angle16::atan2_fast(1, 0), Angle16::QUARTER_TURN);
    assert_eq!(Angle16::atan2_fast(0, -1), Angle16::HALF_TURN);
    assert_eq!(Angle16::atan2_fast(-1, 0), Angle16::THREE_QUARTER_TURN);
    assert_eq!(Angle16::atan2_fast(0, 0), Angle16::ZERO);

    // The half turn is the one place the Q30 accumulator reaches a value that
    // only survives the shift to a 32-bit phase by wrapping through `u32`.
    assert_eq!(Angle32::atan2_fast(0, -1), Angle32::HALF_TURN);
    assert_eq!(Angle32::atan2_fast(0, i32::MIN), Angle32::HALF_TURN);
}

#[test]
fn fast_atan2_survives_extreme_coordinates() {
    // The magnitudes that make a 32-bit intermediate overflow if the shifts are
    // wrong. Run under `cargo test` (debug), where overflow checks are on, this
    // is a proof rather than a smoke test.
    let extremes = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
    for y in extremes {
        for x in extremes {
            let fast = Angle16::atan2_fast(y, x);
            if y != 0 || x != 0 {
                let exact = Angle16::atan2(i64::from(y), i64::from(x));
                let error = exact.abs_diff(fast).to_bits();
                assert!(error <= 46, "atan2_fast({y}, {x}) off by {error} bits");
            }
        }
    }

    // Walk the ratio domain end to end. With a divisor of exactly 2^15 the
    // normalization is a no-op and the quotient is the numerator, so this drives
    // the internal ratio through every value it can take -- and with it the
    // `r * (1 - r)` wedge, whose product with the correction weight is the
    // widest intermediate in the function.
    for y in 0..=32_768 {
        let fast = Angle32::atan2_fast(y, 32_768);
        let exact = Angle32::atan2(i64::from(y), 32_768);
        let limit = (4.4e-3 / core::f64::consts::TAU * 4_294_967_296.0).ceil() as u32;
        let error = exact.abs_diff(fast).to_bits();
        assert!(error <= limit, "atan2_fast({y}, 32768) off by {error} bits");
    }

    let mut rng = Rng::new(0x5f37_1e9d_c084_2b16);
    for _ in 0..200_000 {
        let y = rng.next_u64() as i32;
        let x = rng.next_u64() as i32;
        let fast = Angle32::atan2_fast(y, x);
        if y == 0 && x == 0 {
            continue;
        }
        let exact = Angle32::atan2(i64::from(y), i64::from(x));
        // 4.4e-3 radians in Angle32 bits.
        let limit = (4.4e-3 / core::f64::consts::TAU * 4_294_967_296.0).ceil() as u32;
        let error = exact.abs_diff(fast).to_bits();
        assert!(error <= limit, "atan2_fast({y}, {x}) off by {error} bits");
    }
}

/// The worst case over every phase there is, rather than a sample of them.
///
/// The bound the `_fast` docs quote comes from this sweep. Running it in debug,
/// where overflow checks are on, doubles as the proof that no `i32` intermediate
/// in the 32-bit kernel overflows anywhere in the domain.
///
/// Ignored because it walks all 2^32 phases. Run it with:
///
/// ```sh
/// cargo test -p corvid_fixed --release exhaustively_within -- --ignored
/// ```
#[test]
#[ignore = "walks all 2^32 phases; run explicitly"]
fn fast_sine_is_exhaustively_within_its_bound_for_angle32() {
    let threads = std::thread::available_parallelism().map_or(4, core::num::NonZero::get);
    let span = (1_u64 << 32) / threads as u64;

    let worst = std::thread::scope(|scope| {
        // The `collect` is what makes this parallel: fusing it into the fold
        // below would spawn each thread and immediately join it, walking the
        // domain one slice at a time.
        #[allow(
            clippy::needless_collect,
            reason = "the collect forces every thread to spawn before any is joined"
        )]
        let handles: Vec<_> = (0..threads)
            .map(|slot| {
                scope.spawn(move || {
                    let start = slot as u64 * span;
                    let end = if slot + 1 == threads {
                        1_u64 << 32
                    } else {
                        start + span
                    };
                    let mut worst = 0.0_f64;
                    for phase in start..end {
                        let phase = phase as u32;
                        let expected =
                            (f64::from(phase) / 4_294_967_296.0 * core::f64::consts::TAU).sin();
                        let actual = Angle32::from_bits(phase).sin_fast().to_f64();
                        worst = worst.max((actual - expected).abs());
                    }
                    worst
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(f64::NAN))
            .fold(0.0_f64, f64::max)
    });

    assert!(
        worst <= 1.2e-3,
        "sin_fast worst absolute error {worst:e} exceeds the documented 1.2e-3"
    );
    assert!(
        worst > 1.05e-3,
        "sin_fast improved to {worst:e}; tighten the documented bound"
    );
}

#[test]
fn fast_atan2_is_within_its_documented_error() {
    // 4.4e-3 radians, expressed in Angle16 bits.
    let limit = (4.4e-3 / core::f64::consts::TAU * 65_536.0).ceil() as u16;
    let mut worst = 0_u16;
    for y in -40_i32..=40 {
        for x in -40_i32..=40 {
            let exact = Angle16::atan2(i64::from(y), i64::from(x));
            let fast = Angle16::atan2_fast(y, x);
            worst = worst.max(exact.abs_diff(fast).to_bits());
        }
    }
    assert!(
        worst <= limit,
        "fast atan2 off by {worst} bits (limit {limit})"
    );
}
