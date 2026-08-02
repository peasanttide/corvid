//! Layout guarantees, the wire format, and the optional integrations.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use core::mem::{align_of, size_of};

use corvid_transform::{FineTransform, GlobalPoint, Rotation, Transform};

#[test]
fn the_wire_sizes_are_what_the_docs_claim() {
    assert_eq!((size_of::<Transform>(), align_of::<Transform>()), (16, 4));
    assert_eq!(
        (size_of::<FineTransform>(), align_of::<FineTransform>()),
        (32, 8)
    );
}

#[test]
fn equal_transforms_hash_equally() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(Transform::IDENTITY);
    set.insert(Transform::new(GlobalPoint::ZERO, Rotation::IDENTITY));
    assert_eq!(set.len(), 1);
}

#[cfg(feature = "serde")]
#[test]
fn a_transform_is_three_scalars_and_one_integer_on_the_wire() {
    use corvid_transform::{FineRotation, GlobalFinePoint, I24F8, I48F16};

    // The 16 B and 32 B figures have to mean something over the wire, so assert
    // the serialized form rather than only that a round trip succeeds.
    let t = Transform::new(
        GlobalPoint::new(
            I24F8::from_bits(1),
            I24F8::from_bits(2),
            I24F8::from_bits(3),
        ),
        Rotation::from_bits(7),
    );
    assert_eq!(
        serde_json::to_string(&t).unwrap(),
        r#"{"position":[1,2,3],"rotation":7}"#
    );
    assert_eq!(
        serde_json::from_str::<Transform>(r#"{"position":[1,2,3],"rotation":7}"#).unwrap(),
        t
    );

    let f = FineTransform::new(
        GlobalFinePoint::splat(I48F16::from_bits(1 << 40)),
        FineRotation::from_bits(9),
    );
    let text = serde_json::to_string(&f).unwrap();
    assert_eq!(
        text,
        r#"{"position":[1099511627776,1099511627776,1099511627776],"rotation":9}"#
    );
    assert_eq!(serde_json::from_str::<FineTransform>(&text).unwrap(), f);
}

#[cfg(feature = "bytemuck")]
#[test]
fn transforms_are_plain_old_data() {
    let t = Transform::IDENTITY;
    let bytes: &[u8] = bytemuck::bytes_of(&t);
    assert_eq!(bytes.len(), 16);
    assert_eq!(bytemuck::pod_read_unaligned::<Transform>(bytes), t);

    let f = FineTransform::IDENTITY;
    assert_eq!(bytemuck::bytes_of(&f).len(), 32);
    assert_eq!(
        bytemuck::pod_read_unaligned::<FineTransform>(bytemuck::bytes_of(&f)),
        f
    );
}

#[test]
fn the_crate_re_exports_the_whole_stack() {
    // One name for downstream code to depend on.
    let _: corvid_transform::I24F8 = corvid_transform::I24F8::ONE;
    let _: corvid_transform::Angle32 = corvid_transform::Angle32::QUARTER_TURN;
    let _: corvid_transform::Basis = corvid_transform::Basis::IDENTITY;
    let _: corvid_transform::Versor = corvid_transform::Versor::IDENTITY;
    let _: corvid_transform::Direction = corvid_transform::Direction::ZERO;
    let _: corvid_transform::FinePoint = corvid_transform::FinePoint::ZERO;
    let _ = corvid_transform::fixed::I0F8::ZERO;
    let _ = corvid_transform::vector::GlobalPoint::ZERO;
    let _ = corvid_transform::rotation::Rotation::IDENTITY;
}
