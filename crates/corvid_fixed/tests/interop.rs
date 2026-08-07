//! Layout guarantees and the optional integrations.
//!
//! Each feature's tests are gated on that feature, so this file compiles and
//! passes with any subset enabled. Run with `--all-features` to exercise all of
//! it.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::panic_in_result_fn,
    reason = "these tests use ? for the library calls and assert! for the checks"
)]

use core::mem::{align_of, size_of};

use corvid_fixed::{
    Angle8, Angle16, Angle32, Factor8, Factor16, Factor32, I0F8, I2F30, I8F8, I16F16, I24F8,
    I48F16, Signed8, Signed16, Signed32,
};

#[test]
fn every_type_has_the_layout_of_its_storage_integer() {
    assert_eq!((size_of::<I0F8>(), align_of::<I0F8>()), (1, 1));
    assert_eq!((size_of::<I8F8>(), align_of::<I8F8>()), (2, 2));
    assert_eq!((size_of::<I24F8>(), align_of::<I24F8>()), (4, 4));

    assert_eq!((size_of::<Factor8>(), align_of::<Factor8>()), (1, 1));
    assert_eq!((size_of::<Factor16>(), align_of::<Factor16>()), (2, 2));
    assert_eq!((size_of::<Factor32>(), align_of::<Factor32>()), (4, 4));

    assert_eq!((size_of::<Signed8>(), align_of::<Signed8>()), (1, 1));
    assert_eq!((size_of::<Signed16>(), align_of::<Signed16>()), (2, 2));
    assert_eq!((size_of::<Signed32>(), align_of::<Signed32>()), (4, 4));

    assert_eq!((size_of::<Angle8>(), align_of::<Angle8>()), (1, 1));
    assert_eq!((size_of::<Angle16>(), align_of::<Angle16>()), (2, 2));
    assert_eq!((size_of::<Angle32>(), align_of::<Angle32>()), (4, 4));

    assert_eq!((size_of::<I16F16>(), align_of::<I16F16>()), (4, 4));
    assert_eq!((size_of::<I2F30>(), align_of::<I2F30>()), (4, 4));
    assert_eq!((size_of::<I48F16>(), align_of::<I48F16>()), (8, 8));
}

#[test]
fn options_of_these_types_are_not_niche_optimized() {
    // Every bit pattern is a valid value, which is what makes the bytemuck
    // implementations sound. The cost is that Option has to grow a tag.
    assert!(size_of::<Option<Angle16>>() > size_of::<Angle16>());
}

#[test]
fn default_is_zero() {
    assert_eq!(I24F8::default(), I24F8::ZERO);
    assert_eq!(Factor16::default(), Factor16::ZERO);
    assert_eq!(Signed8::default(), Signed8::ZERO);
    assert_eq!(Angle32::default(), Angle32::ZERO);
}

#[test]
fn equal_values_hash_equally() {
    use std::collections::HashMap;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    // The two encodings of -1.0 are one value, so they must hash alike or a
    // HashMap would hold both.
    let denormal = Signed8::from_bits(-128);
    let canonical = Signed8::MIN;
    assert_eq!(denormal, canonical);
    assert_eq!(hash_of(&denormal), hash_of(&canonical));

    let mut map = HashMap::new();
    map.insert(denormal, "first");
    map.insert(canonical, "second");
    assert_eq!(map.len(), 1, "the denormal opened a second slot");

    let mut angles = HashMap::new();
    angles.insert(Angle16::QUARTER_TURN, 1);
    assert_eq!(angles.get(&Angle16::from_degrees(90.0)), Some(&1));
}

#[cfg(feature = "serde")]
mod serde_interop {
    use super::{Angle16, Factor8, I24F8, Signed8, Signed16};

    #[test]
    fn values_serialize_as_their_raw_integer() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&Factor8::from_bits(128))?, "128");
        assert_eq!(serde_json::to_string(&I24F8::from_f64(1.5))?, "384");
        assert_eq!(serde_json::to_string(&Signed8::MIN)?, "-127");
        assert_eq!(serde_json::to_string(&Angle16::QUARTER_TURN)?, "16384");
        Ok(())
    }

    #[test]
    fn every_16_bit_value_survives_a_json_round_trip() -> Result<(), serde_json::Error> {
        for bits in i16::MIN..=i16::MAX {
            let value = Signed16::from_bits(bits);
            let text = serde_json::to_string(&value)?;
            let parsed: Signed16 = serde_json::from_str(&text)?;
            assert_eq!(parsed.to_bits(), bits, "round trip changed {bits}");
        }
        Ok(())
    }

    #[test]
    fn a_struct_of_these_types_round_trips() -> Result<(), serde_json::Error> {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Pose {
            x: I24F8,
            y: I24F8,
            heading: Angle16,
            throttle: Factor8,
        }

        let pose = Pose {
            x: I24F8::from_f64(-12.5),
            y: I24F8::from_f64(3.25),
            heading: Angle16::from_degrees(45.0),
            throttle: Factor8::from_f64(0.75),
        };
        let text = serde_json::to_string(&pose)?;
        assert_eq!(text, r#"{"x":-3200,"y":832,"heading":8192,"throttle":191}"#);
        assert_eq!(serde_json::from_str::<Pose>(&text)?, pose);
        Ok(())
    }

    #[test]
    fn the_denormal_survives_serialization_verbatim() -> Result<(), serde_json::Error> {
        // Serialization is a bit-level operation, so it does not canonicalize.
        let text = serde_json::to_string(&Signed8::from_bits(-128))?;
        assert_eq!(text, "-128");
        let parsed: Signed8 = serde_json::from_str(&text)?;
        assert_eq!(parsed.to_bits(), -128);
        assert_eq!(parsed, Signed8::MIN);
        Ok(())
    }
}

#[cfg(feature = "bytemuck")]
mod bytemuck_interop {
    use super::{Angle16, Factor8, I24F8, Signed16};

    #[test]
    fn values_cast_to_and_from_bytes() {
        let angle = Angle16::from_degrees(90.0);
        let bytes = bytemuck::bytes_of(&angle);
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytemuck::from_bytes::<Angle16>(bytes), &angle);
    }

    #[test]
    fn slices_cast_without_copying() {
        let factors = [Factor8::from_bits(0), Factor8::from_bits(128), Factor8::MAX];
        let bytes: &[u8] = bytemuck::cast_slice(&factors);
        assert_eq!(bytes, &[0, 128, 255]);

        // And back again, which is what uploading a vertex buffer needs.
        let recovered: &[Factor8] = bytemuck::cast_slice(bytes);
        assert_eq!(recovered, factors.as_slice());
    }

    #[test]
    fn arbitrary_bytes_are_valid_values() {
        // Sound because every bit pattern denotes a value. For Signed16 that
        // includes the denormal, which still reads as -1.0.
        let bytes: [u8; 4] = [0x00, 0x80, 0xFF, 0x7F];
        let values: &[Signed16] = bytemuck::cast_slice(&bytes);
        assert_eq!(values[0], Signed16::MIN);
        assert_eq!(values[0].to_f64(), -1.0);
        assert!(values[0].is_denormal());
        assert_eq!(values[1], Signed16::MAX);
    }

    #[test]
    fn zeroed_memory_is_zero() {
        assert_eq!(<I24F8 as bytemuck::Zeroable>::zeroed(), I24F8::ZERO);
        assert_eq!(<Angle16 as bytemuck::Zeroable>::zeroed(), Angle16::ZERO);
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary_interop {
    use arbitrary::{Arbitrary, Unstructured};

    use super::{Angle16, Factor32, I8F8, Signed8};

    #[test]
    fn values_can_be_generated_from_a_byte_stream() -> arbitrary::Result<()> {
        let data = [0x12_u8, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut source = Unstructured::new(&data);

        let angle = Angle16::arbitrary(&mut source)?;
        let fixed = I8F8::arbitrary(&mut source)?;
        let snorm = Signed8::arbitrary(&mut source)?;

        // Whatever came out must be a usable value, denormal included.
        assert!(angle.to_turns() >= 0.0 && angle.to_turns() < 1.0);
        assert!(fixed.to_f64() >= -128.0 && fixed.to_f64() < 128.0);
        assert!(snorm.to_f64() >= -1.0 && snorm.to_f64() <= 1.0);
        Ok(())
    }

    #[test]
    fn a_long_stream_produces_only_valid_values() -> arbitrary::Result<()> {
        let data: Vec<u8> = (0..=u8::MAX).cycle().take(4096).collect();
        let mut source = Unstructured::new(&data);
        while !source.is_empty() {
            let factor = Factor32::arbitrary(&mut source)?;
            assert!((0.0..=1.0).contains(&factor.to_f64()));
        }
        Ok(())
    }
}

mod digest_interop {
    use core::hash::Hash;
    use std::collections::HashSet;

    use corvid_fixed::{I48F16, Pitch8};
    use corvid_hash::{Digest, Hasher, digest};

    use super::{Angle8, Angle16, Factor8, I0F8, Signed8};

    /// A scalar whose storage integer is already sixty-four bits absorbs one
    /// whole word, which is what `absorb` writes — so this one scalar's digest
    /// can be written out in a `const` context by hand and compared against the
    /// runtime path.
    const DEEP: Digest = Hasher::new().absorb(0x0123_4567_89ab_cdef).digest();

    /// The number of distinct digests over an 8-bit type's whole range.
    fn spread<T: Hash>(of: impl Fn(i8) -> T) -> usize {
        (i8::MIN..=i8::MAX)
            .map(|bits| digest(&of(bits)))
            .collect::<HashSet<Digest>>()
            .len()
    }

    #[test]
    fn const_and_runtime_evaluation_agree() {
        assert_eq!(DEEP, digest(&I48F16::from_bits(0x0123_4567_89ab_cdef)));
    }

    #[test]
    fn a_scalar_absorbs_its_storage_integer_at_its_own_width() {
        // A narrower scalar absorbs as many bytes as its storage integer has,
        // so two families at two widths are two encodings and the same number
        // at two widths is two inputs.
        let mut narrow = Hasher::new();
        Angle16::QUARTER_TURN.to_bits().hash(&mut narrow);
        assert_eq!(narrow.digest(), digest(&Angle16::QUARTER_TURN));
        assert_ne!(
            digest(&Angle16::QUARTER_TURN),
            digest(&u64::from(Angle16::QUARTER_TURN.to_bits()))
        );
    }

    #[test]
    fn every_i0f8_bit_pattern_digests_uniquely() {
        assert_eq!(spread(I0F8::from_bits), 256, "two values shared a digest");
    }

    #[test]
    fn the_other_faithful_families_separate_their_whole_range_too() {
        // These three encode each value exactly once, so all 256 patterns are
        // 256 values and none of them may collide.
        assert_eq!(spread(|bits| Angle8::from_bits(bits.cast_unsigned())), 256);
        assert_eq!(spread(|bits| Factor8::from_bits(bits.cast_unsigned())), 256);
        assert_eq!(spread(Signed8::from_bits), 255, "the denormal is a value");
    }

    #[test]
    fn a_digest_agrees_with_equality_wherever_the_encoding_is_redundant() {
        // Signed8 spends one pattern twice and Pitch8 accepts patterns outside
        // its own range, so both have values with more than one encoding. A
        // digest that told two of them apart would report a desync between two
        // peers holding the same value, which is the one thing it exists to
        // avoid — so it folds exactly where `Eq` and `Hash` do.
        for left in i8::MIN..=i8::MAX {
            for right in i8::MIN..=i8::MAX {
                let (a, b) = (Signed8::from_bits(left), Signed8::from_bits(right));
                assert_eq!(a == b, digest(&a) == digest(&b), "{left} versus {right}");

                let (a, b) = (Pitch8::from_bits(left), Pitch8::from_bits(right));
                assert_eq!(a == b, digest(&a) == digest(&b), "{left} versus {right}");
            }
        }
    }

    #[test]
    fn the_denormal_digests_as_the_value_it_denotes() {
        assert_eq!(digest(&Signed8::from_bits(-128)), digest(&Signed8::MIN));
    }

    #[test]
    fn an_out_of_range_pitch_digests_as_the_pole_it_clamps_to() {
        assert_eq!(digest(&Pitch8::from_bits(i8::MAX)), digest(&Pitch8::MAX));
    }
}

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

/// `nalgebra` needs no code from this crate: its blanket `Scalar` impl already
/// covers these types, and its arithmetic is built on the operator traits.
///
/// These tests exist to keep that true. If a future change broke `Copy`,
/// `PartialEq`, `Debug`, or an operator impl, `Vector3<I24F8>` would stop
/// compiling and this file would say so.
mod nalgebra_interop {
    use nalgebra::{Vector2, Vector3};

    use super::{Angle16, I24F8, Signed16};

    #[test]
    fn vectors_of_fixed_point_add_and_subtract() {
        let a = Vector3::new(
            I24F8::from_f64(1.5),
            I24F8::from_f64(-2.25),
            I24F8::from_f64(0.125),
        );
        let b = Vector3::new(I24F8::ONE, I24F8::ONE, I24F8::ONE);

        let sum = a + b;
        assert_eq!(sum[0].to_f64(), 2.5);
        assert_eq!(sum[1].to_f64(), -1.25);
        assert_eq!(sum[2].to_f64(), 1.125);

        let difference = sum - b;
        assert_eq!(difference, a);
    }

    #[test]
    fn vectors_saturate_component_wise() {
        let a = Vector2::new(I24F8::MAX, I24F8::MIN);
        let b = Vector2::new(I24F8::ONE, I24F8::ONE);
        let sum = a + b;
        assert_eq!(sum[0], I24F8::MAX, "component should have saturated");
        assert_eq!(sum[1].to_f64(), I24F8::MIN.to_f64() + 1.0);
    }

    #[test]
    fn vectors_of_the_other_families_work_too() {
        let normals = Vector3::new(Signed16::MAX, Signed16::ZERO, Signed16::MIN);
        assert_eq!(normals.map(Signed16::to_f64), Vector3::new(1.0, 0.0, -1.0));

        let headings = Vector2::new(Angle16::QUARTER_TURN, Angle16::HALF_TURN);
        let turned = headings + Vector2::new(Angle16::QUARTER_TURN, Angle16::HALF_TURN);
        assert_eq!(turned, Vector2::new(Angle16::HALF_TURN, Angle16::ZERO));
    }

    #[cfg(feature = "num-traits")]
    #[test]
    fn dot_products_work_once_num_traits_supplies_zero() {
        let a = Vector3::new(I24F8::from_f64(1.0), I24F8::from_f64(2.0), I24F8::ZERO);
        let b = Vector3::new(I24F8::from_f64(3.0), I24F8::from_f64(4.0), I24F8::ONE);
        assert_eq!(a.dot(&b).to_f64(), 11.0);
    }
}
