//! The whole output: one struct per live particle, per frame.

/// One particle as a renderer wants it: forty bytes, `Pod`, and nothing in it
/// that has to be looked up anywhere else.
///
/// | | Bytes | Offset |
/// |---|---|---|
/// | [`position`](Self::position) | 12 | 0 |
/// | [`size`](Self::size) | 4 | 12 |
/// | [`color`](Self::color) | 16 | 16 |
/// | [`rotation`](Self::rotation) | 4 | 32 |
/// | [`age`](Self::age) | 4 | 36 |
///
/// The fields are in that order so the first sixteen bytes are a position and a
/// size and the second sixteen are a colour, which is two aligned reads in a
/// shader. There is no padding, which is what lets `bytemuck::cast_slice` turn
/// a slice of these into the bytes of an instance buffer -- the same reason
/// `corvid_mesh::Vertex` is `Pod` whatever the features say.
///
/// **There is no velocity here**, and a renderer that wants a streak rather
/// than a billboard gets it from a trail rather than from a stretched quad. See
/// [`Trail`](crate::Trail).
///
/// ```
/// use corvid_particle::Instance;
///
/// assert_eq!(size_of::<Instance>(), 40);
/// assert_eq!(align_of::<Instance>(), 4);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// Where it is, in the system's frame, in metres.
    pub position: [f32; 3],
    /// How big it is, in metres, after the growth over its life.
    pub size: f32,
    /// What colour it is now: linear, straight rather than premultiplied, in
    /// `r`, `g`, `b`, `a` order.
    ///
    /// Straight because that is what [`corvid_color::LinearRgba`] holds and
    /// which blend a game wants is the game's decision;
    /// `corvid_color::LinearRgba::premultiplied` is the other one.
    pub color: [f32; 4],
    /// How far it has turned about the axis it faces, in radians.
    ///
    /// A particle has no orientation of its own because it is drawn as a quad
    /// facing the camera, and the one freedom such a quad has is the roll.
    pub rotation: f32,
    /// How far through its life it is: zero at birth, one at death.
    ///
    /// The colour and the size already have this applied. It is here for what
    /// only a shader can do with it -- picking a frame out of a flipbook, or
    /// fading a soft edge -- and because it is what a golden test names a
    /// particle by when the position has moved.
    pub age: f32,
}
