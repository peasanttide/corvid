//! A unit direction in sixteen bits, octahedrally encoded.
//!
//! [`Direction`] spends twelve bytes on a unit vector, which is right for a
//! rotation axis and wrong for a mesh vertex: a normal is one of three or four
//! attributes on every vertex of every mesh, and twelve bytes there is most of
//! the vertex. [`OctDirection`] is the storage form — two bytes, decoded once
//! per use.
//!
//! # The map
//!
//! Project the unit sphere onto the octahedron `|x| + |y| + |z| = 1` by dividing
//! by `|x| + |y| + |z|`; the upper hemisphere's four faces then unfold onto the
//! diamond `|u| + |v| <= 1`, and the lower four fold *outward* into the four
//! corners of the square `[-1, 1]²`. The square is what gets quantized, and it
//! is nearly area-preserving, which is why an octahedral encoding beats
//! spherical coordinates at the same width — a latitude/longitude pair spends
//! most of its resolution near the poles, where the sphere has least area.
//!
//! The outward fold is what makes the encoding continuous across the `z = 0`
//! seam and across the outer edges of the square: walking over the diamond's
//! boundary walks off one face and onto its neighbour, and walking off the edge
//! of the square re-enters at the mirrored point on the same edge. A naive
//! implementation that clamps instead is worst exactly there, which is why the
//! tests sample the seams on purpose rather than trusting random directions to
//! land on them.

use corvid_fixed::{Signed8, Signed32};

use crate::Direction;

/// The bit pattern of `1.0` in a [`Signed8`] component, which is the scale both
/// halves of the codec work on.
const UNIT: i64 = 127;

/// A unit direction packed into **two bytes**, octahedrally encoded.
///
/// | | |
/// |---|---|
/// | Storage | two [`Signed8`], `SNORM8x2` |
/// | Worst measured error | 0.9569° |
/// | Mean measured error | 0.3370° over the sphere |
///
/// Both figures are measured by `tests/oct.rs`, which also derives the worst
/// one. The worst place is the centre of an octahedron face, where the
/// octahedron comes nearest the origin and a grid step therefore subtends the
/// most angle: the half-diagonal of a quantization cell there is `√1.5/127`,
/// the radius is `1/√3`, and the quotient is `√4.5/127` radians — 0.95703°,
/// against 0.956822° measured. The README has the seam's own bound and the
/// outer edges', which are lower and are checked separately.
///
/// The two components are the octahedral `u` and `v` — *not* two components of
/// the direction — so reading them individually is only useful to something that
/// knows the mapping. [`decode`](Self::decode) is how a caller gets a direction
/// back, and it is integer-only and deterministic like everything else here.
///
/// # Why two `Signed8` rather than one `u16`
///
/// This is a mesh vertex format, so the thing that matters is what a GPU can
/// read without unpacking it: two `Signed8` laid out `#[repr(C)]` *is*
/// `wgpu`'s `Snorm8x2` vertex format, byte for byte, so a vertex buffer holds
/// these directly and a shader gets a `vec2<f32>` in `[-1, 1]` for free. A
/// `u16` would be the same sixteen bits and would need a shift, a mask and a
/// bias in every vertex shader that touched it. It is the same argument that
/// keeps [`Direction`] on [`Signed32`] rather than moving it to `I2F30`.
///
/// # Examples
///
/// ```
/// use corvid_vector::{Direction, OctDirection, direction};
/// use corvid_fixed::Signed32;
///
/// // The axes survive exactly: they are corners of the octahedron, so the
/// // encoding lands on them rather than near them.
/// let up = direction(Signed32::ZERO, Signed32::ZERO, Signed32::MAX);
/// assert_eq!(OctDirection::encode(up).decode(), up);
///
/// // Two bytes, and that is the whole point.
/// assert_eq!(core::mem::size_of::<OctDirection>(), 2);
/// assert_eq!(core::mem::size_of::<Direction>(), 12);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "arbitrary", derive(::arbitrary::Arbitrary))]
#[cfg_attr(feature = "bytemuck", derive(::bytemuck::Pod, ::bytemuck::Zeroable))]
pub struct OctDirection([Signed8; 2]);

impl OctDirection {
    /// The encoding of **+Z**, the crate's up.
    ///
    /// This is also the all-zero bit pattern, so a zeroed vertex buffer holds
    /// normals pointing up rather than nothing at all — [`Default`] and
    /// `bytemuck::zeroed` agree with it.
    pub const UP: Self = Self([Signed8::ZERO; 2]);

    /// Builds an encoded direction from its two octahedral components.
    ///
    /// Every pair of components names some direction, so this cannot fail — but
    /// it also does no encoding, so unless the two came out of
    /// [`to_array`](Self::to_array) the result is unlikely to be the direction
    /// the caller had in mind. [`encode`](Self::encode) is the constructor to
    /// reach for.
    #[must_use]
    #[inline]
    pub const fn new(u: Signed8, v: Signed8) -> Self {
        Self([u, v])
    }

    /// The two octahedral components.
    #[must_use]
    #[inline]
    pub const fn to_array(self) -> [Signed8; 2] {
        self.0
    }

    /// The first octahedral component.
    #[must_use]
    #[inline]
    pub const fn u(self) -> Signed8 {
        self.0[0]
    }

    /// The second octahedral component.
    #[must_use]
    #[inline]
    pub const fn v(self) -> Signed8 {
        self.0[1]
    }

    /// Encodes a unit direction.
    ///
    /// Integer-only: one sum, two multiplications by 127 and two rounded
    /// divisions, all in `i64`. Only the ratios of the components matter, so an
    /// unnormalized vector encodes as the direction it points in — the division
    /// by `|x| + |y| + |z|` is the projection onto the octahedron and it
    /// normalizes as a side effect.
    ///
    /// [`Direction::ZERO`] names no direction; it encodes as
    /// [`UP`](Self::UP), which is what `decode` of the zero pattern gives back.
    #[must_use]
    #[inline]
    pub const fn encode(direction: Direction) -> Self {
        let [x, y, z] = direction.to_array();
        let (x, y, z) = (
            x.canonicalize().to_bits() as i64,
            y.canonicalize().to_bits() as i64,
            z.canonicalize().to_bits() as i64,
        );
        let (a, b, c) = (x.abs(), y.abs(), z.abs());
        let sum = a + b + c;
        if sum == 0 {
            return Self::UP;
        }

        // The upper hemisphere is the projection itself. The lower one folds
        // outward: each coordinate becomes the *other* one's distance from the
        // diamond's edge, carrying its own sign, with zero counting as positive
        // so that the four faces meet rather than overlap.
        let (nu, nv) = if z >= 0 {
            (x, y)
        } else {
            (sign_not_zero(x) * (sum - b), sign_not_zero(y) * (sum - a))
        };

        Self([quantize(nu, sum), quantize(nv, sum)])
    }

    /// Decodes back to a unit direction.
    ///
    /// Integer-only, and it finishes through the same
    /// [`normalize`](Direction::normalize) every other direction in this crate
    /// is built by, so the result is a [`Direction`] to the last bit rather than
    /// merely close to one. What the round trip costs is the quantization of the
    /// square, which is the number in this type's table.
    #[must_use]
    #[inline]
    pub const fn decode(self) -> Direction {
        let u = self.0[0].canonicalize().to_bits() as i128;
        let v = self.0[1].canonicalize().to_bits() as i128;
        // Height above the diamond, at the same scale. Negative exactly inside
        // the four corners, which is where the lower hemisphere went.
        let w = UNIT as i128 - u.abs() - v.abs();
        let (x, y) = if w < 0 {
            (
                sign_not_zero_wide(u) * (UNIT as i128 - v.abs()),
                sign_not_zero_wide(v) * (UNIT as i128 - u.abs()),
            )
        } else {
            (u, v)
        };

        match crate::point::normalize_bits([x, y, w], false) {
            Some(direction) => direction,
            // `w` is zero only when `|u| + |v|` is 127, and then at least one of
            // the two is non-zero — so the three are never all zero and the
            // normalize never fails. Naming the answer beats naming a panic.
            None => Direction::new(Signed32::ZERO, Signed32::ZERO, Signed32::MAX),
        }
    }
}

/// `1` for a positive value **or zero**, `-1` for a negative one.
///
/// Zero counting as positive is not a detail: the fold sends `(x, y)` to
/// `(sign(x)·…, sign(y)·…)`, and a `sign` that answered zero at zero would
/// collapse a whole edge of the lower hemisphere onto the origin, which decodes
/// to **+Z** — a normal pointing the wrong way along a seam. Encoder and
/// decoder use the same convention, which is what makes the fold its own
/// inverse.
#[inline]
const fn sign_not_zero(value: i64) -> i64 {
    if value < 0 { -1 } else { 1 }
}

/// [`sign_not_zero`] at the width the decoder works in.
#[inline]
const fn sign_not_zero_wide(value: i128) -> i128 {
    if value < 0 { -1 } else { 1 }
}

/// `round(127 · numerator / denominator)`, halfway away from zero.
///
/// `denominator` is the sum of the three absolute components and is positive;
/// `|numerator|` never exceeds it, so the result is always inside
/// `-127 ..= 127` and the [`Signed8`] it becomes is never the denormal.
#[inline]
const fn quantize(numerator: i64, denominator: i64) -> Signed8 {
    let scaled = numerator * UNIT;
    let rounded = if scaled >= 0 {
        (2 * scaled + denominator) / (2 * denominator)
    } else {
        -((-2 * scaled + denominator) / (2 * denominator))
    };
    Signed8::from_bits(rounded as i8)
}

/// Decoding, and deliberately not encoding.
///
/// There is no `From<Direction>` here, and its absence is the same rule
/// `corvid_fixed`'s README states about integers and `from_f64`: **a conversion
/// that rounds says so at the call site rather than happening because a value
/// was passed to something.** [`encode`](OctDirection::encode) quantizes a
/// direction onto a 127-step grid and costs it up to 0.9569° — a `.into()` on
/// the way into a vertex buffer would be the whole of that loss, written
/// nowhere.
///
/// This direction rounds nothing the caller can see. The code is sixteen bits
/// and this is the direction those sixteen bits name; there is no other, and
/// nothing is being chosen between.
impl From<OctDirection> for Direction {
    #[inline]
    fn from(encoded: OctDirection) -> Self {
        encoded.decode()
    }
}

/// The two octahedral components, which is [`OctDirection::to_array`].
impl From<OctDirection> for [Signed8; 2] {
    #[inline]
    fn from(encoded: OctDirection) -> Self {
        encoded.to_array()
    }
}

/// The code those two components are, which is [`OctDirection::new`].
///
/// Nothing is encoded and nothing rounds: every pair of components names some
/// direction, so this is the same reinterpretation `new` is rather than a
/// conversion from a direction, which is [`encode`](OctDirection::encode) and
/// deliberately has no `From`.
impl From<[Signed8; 2]> for OctDirection {
    #[inline]
    fn from([u, v]: [Signed8; 2]) -> Self {
        Self::new(u, v)
    }
}

impl core::fmt::Debug for OctDirection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "OctDirection(u {}, v {} -> {:?})",
            self.0[0].to_f64(),
            self.0[1].to_f64(),
            self.decode()
        )
    }
}

impl core::fmt::Display for OctDirection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.decode(), f)
    }
}
