//! The `num-traits` integration.
//!
//! The point of implementing these traits is that a caller can be generic over
//! a scalar, so what is checked is that each trait does what the inherent
//! method of the same name does -- and that the checked and wrapping families
//! keep their own semantics when reached through a trait bound.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::panic_in_result_fn,
    reason = "these tests use ? for the library calls and assert! for the checks"
)]

use corvid_fixed::{Angle16, Factor16, I8F8, I24F8, Signed16};
#[cfg(feature = "num-traits")]
mod num_traits_interop {
    use num_traits::{
        Bounded, CheckedAdd, CheckedDiv, CheckedMul, CheckedSub, FromPrimitive, One, Saturating,
        SaturatingAdd, SaturatingMul, SaturatingSub, ToPrimitive, WrappingAdd, WrappingNeg,
        WrappingSub, Zero,
    };

    use super::{Angle16, Factor16, I8F8, I24F8, Signed16};

    // Every call below goes through the trait explicitly. The inherent methods
    // have the same names and would otherwise win method resolution, which would
    // leave the trait implementations untested.

    /// Generic over anything that can accumulate, to prove the traits compose.
    fn sum_all<T: Zero + SaturatingAdd + Copy>(values: &[T]) -> T {
        values
            .iter()
            .fold(T::zero(), |acc, v| SaturatingAdd::saturating_add(&acc, v))
    }

    /// The same, for the families that wrap rather than saturate.
    fn turn_all<T: Zero + WrappingAdd + Copy>(values: &[T]) -> T {
        values
            .iter()
            .fold(T::zero(), |acc, v| WrappingAdd::wrapping_add(&acc, v))
    }

    #[test]
    fn zero_and_one_are_the_arithmetic_identities() {
        assert_eq!(<I24F8 as Zero>::zero(), I24F8::ZERO);
        assert!(Zero::is_zero(&I24F8::ZERO));
        assert_eq!(<I24F8 as One>::one(), I24F8::ONE);
        assert_eq!(<Factor16 as One>::one(), Factor16::MAX);
        assert_eq!(<Signed16 as One>::one(), Signed16::MAX);
        assert!(!Zero::is_zero(&Angle16::QUARTER_TURN));
        assert!(Zero::is_zero(&<Angle16 as Zero>::zero()));
    }

    #[test]
    fn bounds_match_the_inherent_constants() {
        assert_eq!(<I8F8 as Bounded>::min_value(), I8F8::MIN);
        assert_eq!(<I8F8 as Bounded>::max_value(), I8F8::MAX);
        assert_eq!(<Signed16 as Bounded>::min_value(), Signed16::MIN);
        assert_eq!(<Factor16 as Bounded>::min_value(), Factor16::ZERO);
        assert_eq!(<Angle16 as Bounded>::max_value(), Angle16::MAX);
    }

    #[test]
    fn primitive_conversions_agree_with_the_inherent_ones() {
        let value = I8F8::from_f64(-2.5);
        assert_eq!(ToPrimitive::to_f64(&value), Some(-2.5));
        assert_eq!(ToPrimitive::to_f32(&value), Some(-2.5));
        assert_eq!(ToPrimitive::to_i64(&value), Some(-2));
        assert_eq!(ToPrimitive::to_u64(&value), None);
        assert_eq!(ToPrimitive::to_u64(&I8F8::ONE), Some(1));

        assert_eq!(
            <I8F8 as FromPrimitive>::from_i64(3),
            Some(I8F8::from_f64(3.0))
        );
        assert_eq!(<I8F8 as FromPrimitive>::from_i64(1_000_000), None);
        assert_eq!(
            <I8F8 as FromPrimitive>::from_u64(3),
            Some(I8F8::from_f64(3.0))
        );
        assert_eq!(
            <Factor16 as FromPrimitive>::from_f64(0.5),
            Some(Factor16::from_f64(0.5))
        );
        assert_eq!(<Factor16 as FromPrimitive>::from_i64(2), None);

        // An angle reads as turns, so a whole turn is out of range.
        assert_eq!(ToPrimitive::to_f64(&Angle16::QUARTER_TURN), Some(0.25));
        assert_eq!(<Angle16 as FromPrimitive>::from_i64(1), None);
    }

    #[test]
    fn checked_traits_report_overflow() {
        assert_eq!(CheckedAdd::checked_add(&I8F8::MAX, &I8F8::ONE), None);
        assert_eq!(CheckedSub::checked_sub(&I8F8::MIN, &I8F8::ONE), None);
        assert_eq!(CheckedMul::checked_mul(&I8F8::MAX, &I8F8::MAX), None);
        assert_eq!(CheckedDiv::checked_div(&I8F8::ONE, &I8F8::ZERO), None);
        assert_eq!(
            CheckedAdd::checked_add(&I8F8::ONE, &I8F8::ONE),
            Some(I8F8::from_f64(2.0))
        );
        assert_eq!(
            CheckedAdd::checked_add(&Signed16::MAX, &Signed16::MAX),
            None
        );
        // Multiplication over the unit interval cannot fail.
        assert_eq!(
            CheckedMul::checked_mul(&Factor16::MAX, &Factor16::MAX),
            Some(Factor16::MAX)
        );
    }

    #[test]
    fn saturating_traits_clamp() {
        assert_eq!(Saturating::saturating_add(I8F8::MAX, I8F8::ONE), I8F8::MAX);
        assert_eq!(Saturating::saturating_sub(I8F8::MIN, I8F8::ONE), I8F8::MIN);
        assert_eq!(
            SaturatingAdd::saturating_add(&I8F8::MAX, &I8F8::ONE),
            I8F8::MAX
        );
        assert_eq!(
            SaturatingSub::saturating_sub(&Factor16::ZERO, &Factor16::ONE),
            Factor16::ZERO
        );
        assert_eq!(
            SaturatingMul::saturating_mul(&I8F8::MAX, &I8F8::MAX),
            I8F8::MAX
        );
    }

    #[test]
    fn wrapping_traits_exist_for_the_modular_families() {
        assert_eq!(
            WrappingAdd::wrapping_add(&I8F8::MAX, &I8F8::DELTA),
            I8F8::MIN
        );
        assert_eq!(
            WrappingSub::wrapping_sub(&I8F8::MIN, &I8F8::DELTA),
            I8F8::MAX
        );
        assert_eq!(
            WrappingAdd::wrapping_add(&Angle16::MAX, &Angle16::DELTA),
            Angle16::ZERO
        );
        assert_eq!(
            WrappingNeg::wrapping_neg(&Angle16::QUARTER_TURN),
            Angle16::THREE_QUARTER_TURN
        );
    }

    #[test]
    fn the_traits_compose_in_generic_code() {
        let fixed = [I24F8::from_f64(1.5), I24F8::from_f64(2.25), I24F8::ONE];
        assert_eq!(sum_all(&fixed).to_f64(), 4.75);

        let factors = [Factor16::from_f64(0.25); 8];
        assert_eq!(sum_all(&factors), Factor16::ONE, "should saturate at one");

        // Angles have no saturating form, so they accumulate by wrapping.
        let angles = [Angle16::QUARTER_TURN; 7];
        assert_eq!(turn_all(&angles), Angle16::THREE_QUARTER_TURN);
    }
}
