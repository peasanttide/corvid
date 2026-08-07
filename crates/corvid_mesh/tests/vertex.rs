//! The twelve bytes, frozen.
//!
//! A mesh is a wire format the moment one is written down: a game that
//! generates geometry into a file, an asset pipeline that bakes one, a golden
//! that records what a build produced. So the twelve bytes are pinned here as
//! bytes, not as a `size_of` — a struct that kept its size while its fields
//! moved would pass a size assertion and load every mesh in the world wrong.
//!
//! The layout the device reads is pinned beside them in
//! `corvid_mesh_render/tests/layout.rs`, because the two have to agree and
//! nothing in the type system says so: a field reordered in Rust and not in the
//! layout is a mesh that renders as noise, and neither half alone would notice.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_fixed::Signed32;
use corvid_mesh::Vertex;
use corvid_vector::Direction;

use corvid_vector::OctDirection;
/// The pair of vertices every assertion here is about.
///
/// A corner at full deflection on two axes and zero on the third, with a
/// normal that is not the zero pattern — so a byte that moved shows up as a
/// byte that moved rather than being hidden by a field that is all zeroes
/// anyway.
const fn frozen() -> [Vertex; 2] {
    let slanted = OctDirection::encode(Direction::new(
        Signed32::from_f64(0.6),
        Signed32::from_f64(0.0),
        Signed32::from_f64(-0.8),
    ));
    [
        Vertex::new([Vertex::FULL, -Vertex::FULL, 0], slanted),
        Vertex::new([0, 1, -1], OctDirection::UP),
    ]
}

#[test]
#[cfg(feature = "bytemuck")]
fn a_vertex_is_twelve_bytes_and_these_are_the_bytes() {
    // Little-endian on every target, which is what the whole workspace's
    // encoding is and what a `Snorm16` attribute is read as.
    //
    // Reading right to left: three `i16` positions, the fourth `i16` that is
    // padding because there is no `Snorm16x3`, two bytes of octahedral normal,
    // and two bytes bringing the stride to a multiple of four.
    #[rustfmt::skip]
    const RECORDED: [u8; 24] = [
        0xff, 0x7f,  0x01, 0x80,  0x00, 0x00,  0x00, 0x00,  0x7f, 0x49,  0x00, 0x00,
        0x00, 0x00,  0x01, 0x00,  0xff, 0xff,  0x00, 0x00,  0x00, 0x00,  0x00, 0x00,
    ];

    let vertices = frozen();
    assert_eq!(bytemuck::cast_slice::<Vertex, u8>(&vertices), &RECORDED);

    // Twelve, and against the twenty-four the float vertex this replaced cost:
    // three `f32` of position and three of normal. The comparison is the whole
    // reason the type is shaped this way, so it is a number here rather than a
    // sentence in a README.
    assert_eq!(size_of::<Vertex>(), 12);
    assert_eq!(size_of::<[f32; 3]>() * 2, 24);
}

#[test]
fn the_accessors_read_back_what_was_written() {
    // A byte golden pins the encoding and says nothing about whether the type
    // can be read. This is the other direction, and it is what would fail if
    // `position` returned the padding component instead of one of the three.
    let [slanted, small] = frozen();
    assert_eq!(slanted.position(), [Vertex::FULL, -Vertex::FULL, 0]);
    assert_eq!(small.position(), [0, 1, -1]);
    assert_eq!(small.normal(), OctDirection::UP);
    assert_ne!(slanted.normal(), OctDirection::UP);

    // The normal survived as a direction rather than merely as two bytes: it
    // decodes back to something pointing the way it was encoded from, to within
    // the codec's own worst error of 0.96°.
    let decoded = slanted.normal().decode().to_array();
    assert!(decoded[0].to_f64() > 0.5, "{decoded:?}");
    assert!(decoded[2].to_f64() < -0.7, "{decoded:?}");
}
