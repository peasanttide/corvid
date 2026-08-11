//! What a camera is, as bytes.
//!
//! Every type here is `#[repr(C)]` and `Pod`, which means its layout is a
//! contract rather than an implementation detail: a game writes one into a
//! uniform or storage buffer and a shader reads it back with a matching
//! declaration. A field reordered or a pad word absorbed is a silently wrong
//! picture, so the offsets are asserted rather than assumed.

#![cfg(feature = "bytemuck")]

use core::mem::{align_of, offset_of, size_of};

use corvid_camera::{Eye, FirstPerson, Orbit};
use corvid_vector::GlobalPoint;

use corvid_shape::Frustum;
/// Four `I16F16`s, and -- unlike the `Angle16`-based projection it replaced -- no
/// pad word. That is a side effect of the tagless representation worth having:
/// storing the *law* of the half-height rather than an angle makes every field
/// the same width.
#[test]
fn a_frustum_is_four_words_and_nothing_else() {
    assert_eq!(size_of::<Frustum>(), 16);
    assert_eq!(align_of::<Frustum>(), 4);
    assert_eq!(offset_of!(Frustum, near), 0);
    assert_eq!(offset_of!(Frustum, far), 4);
    assert_eq!(offset_of!(Frustum, base), 8);
    assert_eq!(offset_of!(Frustum, slope), 12);

    // It is `corvid_shape`'s, which is what makes it a volume a game can cull
    // against rather than a camera's private detail. The camera crate no longer
    // re-exports it: `corvid` is the one facade, so a game names that.
}

/// The uniform block, unchanged in layout by the move out of `corvid_render`
/// and by the switch to nalgebra: a `Matrix4<f32>` is the same sixty-four bytes
/// the array of arrays was.
#[test]
fn an_eye_is_what_a_uniform_buffer_takes() {
    assert_eq!(size_of::<Eye>(), 80);
    assert_eq!(align_of::<Eye>(), 4);
    assert_eq!(offset_of!(Eye, coarse), 0);
    assert_eq!(offset_of!(Eye, _pad), 12);
    assert_eq!(offset_of!(Eye, clip), 16);
}

/// `FineRotation` is a `u64`, so a camera holding one is eight-byte aligned and
/// the four-byte fields around it leave gaps. `Pod` forbids a gap a reader
/// cannot see, so each is a named field -- and this is what would fail if one
/// were ever removed or a field inserted in front of it.
#[test]
fn the_cameras_have_no_padding_a_reader_cannot_see() {
    assert_eq!(align_of::<Orbit>(), 8);
    assert_eq!(size_of::<Orbit>(), 56);
    assert_eq!(offset_of!(Orbit, anchor), 0);
    assert_eq!(offset_of!(Orbit, _pad), 12);
    assert_eq!(offset_of!(Orbit, facing), 16);
    assert_eq!(offset_of!(Orbit, offset), 24);
    assert_eq!(offset_of!(Orbit, pitch_limit), 36);
    assert_eq!(offset_of!(Orbit, frustum), 40);

    assert_eq!(align_of::<FirstPerson>(), 8);
    assert_eq!(size_of::<FirstPerson>(), 48);
    assert_eq!(offset_of!(FirstPerson, position), 0);
    assert_eq!(offset_of!(FirstPerson, _pad), 12);
    assert_eq!(offset_of!(FirstPerson, facing), 16);
    assert_eq!(offset_of!(FirstPerson, pitch_limit), 24);
    assert_eq!(offset_of!(FirstPerson, frustum), 28);
    assert_eq!(offset_of!(FirstPerson, _tail), 44);
}

/// The point of it being `Pod`: bytes out, bytes back, no `unsafe` at the call
/// site -- which the workspace forbids anyway.
#[test]
fn a_camera_round_trips_through_its_own_bytes() {
    let camera = Orbit::default();
    let bytes = bytemuck::bytes_of(&camera);
    assert_eq!(bytes.len(), size_of::<Orbit>());
    assert_eq!(bytemuck::from_bytes::<Orbit>(bytes), &camera);

    let walker = FirstPerson::default();
    assert_eq!(
        bytemuck::from_bytes::<FirstPerson>(bytemuck::bytes_of(&walker)),
        &walker
    );

    let lens = Frustum::default();
    assert_eq!(
        bytemuck::from_bytes::<Frustum>(bytemuck::bytes_of(&lens)),
        &lens
    );
}

/// A zeroed camera is a valid one, which is what `Zeroable` claims and what a
/// buffer cleared before its first write actually contains.
#[test]
fn a_zeroed_camera_is_a_camera() {
    let blank: Orbit = bytemuck::Zeroable::zeroed();
    assert_eq!(blank.anchor, GlobalPoint::ZERO);
    assert!(blank.frustum.is_orthographic(), "a zero slope is a box");
}
