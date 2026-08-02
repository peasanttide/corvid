//! Layout guarantees, the wire format, and the optional integrations.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
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

use core::mem::{align_of, size_of};

use corvid_rotation::{Basis, FineRotation, Rotation, Versor};

#[test]
fn the_sizes_are_what_the_docs_claim() {
    assert_eq!((size_of::<Rotation>(), align_of::<Rotation>()), (4, 4));
    assert_eq!(
        (size_of::<FineRotation>(), align_of::<FineRotation>()),
        (8, 8)
    );
    assert_eq!((size_of::<Versor>(), align_of::<Versor>()), (16, 4));
    assert_eq!((size_of::<Basis>(), align_of::<Basis>()), (36, 4));
}

#[cfg(feature = "serde")]
#[test]
fn packed_rotations_serialize_as_bare_integers() {
    // This is what makes corvid_transform's 16 B and 32 B figures mean
    // something over the wire, so assert the serialized form rather than only
    // that a round trip succeeds.
    let r = Rotation::from_bits(0xDEAD_BEEF);
    assert_eq!(serde_json::to_string(&r).unwrap(), "3735928559");
    assert_eq!(serde_json::from_str::<Rotation>("3735928559").unwrap(), r);

    let f = FineRotation::from_bits(0x0123_4567_89AB_CDEF);
    assert_eq!(serde_json::to_string(&f).unwrap(), "81985529216486895");
    assert_eq!(
        serde_json::from_str::<FineRotation>("81985529216486895")
            .unwrap()
            .to_bits(),
        f.to_bits()
    );

    // Four bytes and eight bytes on the wire, not a struct of named fields.
    assert_eq!(
        serde_json::to_string(&Rotation::IDENTITY)
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        Rotation::IDENTITY.to_bits()
    );
}

#[cfg(feature = "bytemuck")]
#[test]
fn the_working_types_are_plain_old_data() {
    let m = Basis::IDENTITY;
    let bytes: &[u8] = bytemuck::bytes_of(&m);
    assert_eq!(bytes.len(), 36);
    assert_eq!(bytemuck::pod_read_unaligned::<Basis>(bytes), m);

    let q = Versor::IDENTITY;
    assert_eq!(bytemuck::bytes_of(&q).len(), 16);
    assert_eq!(bytemuck::bytes_of(&Rotation::IDENTITY).len(), 4);
    assert_eq!(bytemuck::bytes_of(&FineRotation::IDENTITY).len(), 8);
}

#[cfg(feature = "mint")]
#[test]
fn mint_round_trips_a_versor() {
    let mut rng = common::Rng::new(0x1717_1717);
    for _ in 0..1_000 {
        let q = common::random_versor(&mut rng);
        let m: mint::Quaternion<f64> = q.into();
        let back = Versor::from(m);
        assert!(
            q.angle_to(back).to_degrees() < 0.01,
            "{q:?} became {back:?}"
        );
    }
}

#[cfg(feature = "nalgebra")]
#[test]
fn nalgebra_agrees_on_the_matrix_and_the_quaternion() {
    let mut rng = common::Rng::new(0x4A19_4A19);
    for _ in 0..1_000 {
        let q = common::random_versor(&mut rng);
        let m: nalgebra::Matrix3<f64> = q.to_basis().into();
        let u: nalgebra::UnitQuaternion<f64> = q.into();

        // nalgebra builds the same matrix from the same rotation.
        let reference = u.to_rotation_matrix();
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (m[(i, j)] - reference[(i, j)]).abs() < 1e-6,
                    "entry ({i}, {j}): {} vs {}",
                    m[(i, j)],
                    reference[(i, j)]
                );
            }
        }

        assert!(q.angle_to(Versor::from(u)).to_degrees() < 0.01);
    }
}
