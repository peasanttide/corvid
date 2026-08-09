//! Layout guarantees, and the integrations that follow from them.
//!
//! Every type here is a newtype over an integer and has to stay one: `bytemuck`
//! reads it as bytes and `serde` writes it as the number inside, and both stop
//! being sound the moment a niche or a padding byte appears. The `num-traits`
//! and `nalgebra` integrations have their own files.

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
