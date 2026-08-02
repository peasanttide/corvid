//! The optional `mint` and `nalgebra` integrations.
//!
//! Boundary conversions, and therefore the one place in this crate where
//! floating point appears. They go through `f64`, so each component rounds
//! once.

use corvid_fixed::I2F30;

#[cfg(feature = "nalgebra")]
use crate::Basis;
use crate::Versor;

#[cfg(feature = "mint")]
impl From<Versor> for ::mint::Quaternion<f64> {
    #[inline]
    fn from(q: Versor) -> Self {
        let [x, y, z, w] = q.to_xyzw();
        Self {
            v: ::mint::Vector3 {
                x: x.to_f64(),
                y: y.to_f64(),
                z: z.to_f64(),
            },
            s: w.to_f64(),
        }
    }
}

#[cfg(feature = "mint")]
impl From<::mint::Quaternion<f64>> for Versor {
    /// Renormalizes on the way in, so a quaternion that drifted in `f64` still
    /// arrives as a unit versor.
    #[inline]
    fn from(q: ::mint::Quaternion<f64>) -> Self {
        Self::from_xyzw_unchecked([
            I2F30::from_f64(q.v.x).to_bits(),
            I2F30::from_f64(q.v.y).to_bits(),
            I2F30::from_f64(q.v.z).to_bits(),
            I2F30::from_f64(q.s).to_bits(),
        ])
        .renormalize()
    }
}

#[cfg(feature = "nalgebra")]
impl From<Basis> for ::nalgebra::Matrix3<f64> {
    #[inline]
    fn from(m: Basis) -> Self {
        let r = m.to_rows();
        Self::new(
            r[0][0].to_f64(),
            r[0][1].to_f64(),
            r[0][2].to_f64(),
            r[1][0].to_f64(),
            r[1][1].to_f64(),
            r[1][2].to_f64(),
            r[2][0].to_f64(),
            r[2][1].to_f64(),
            r[2][2].to_f64(),
        )
    }
}

#[cfg(feature = "nalgebra")]
impl From<Versor> for ::nalgebra::UnitQuaternion<f64> {
    #[inline]
    fn from(q: Versor) -> Self {
        let [x, y, z, w] = q.to_xyzw();
        Self::new_normalize(::nalgebra::Quaternion::new(
            w.to_f64(),
            x.to_f64(),
            y.to_f64(),
            z.to_f64(),
        ))
    }
}

#[cfg(feature = "nalgebra")]
impl From<::nalgebra::UnitQuaternion<f64>> for Versor {
    #[inline]
    fn from(q: ::nalgebra::UnitQuaternion<f64>) -> Self {
        Self::from_xyzw_unchecked([
            I2F30::from_f64(q.i).to_bits(),
            I2F30::from_f64(q.j).to_bits(),
            I2F30::from_f64(q.k).to_bits(),
            I2F30::from_f64(q.w).to_bits(),
        ])
        .renormalize()
    }
}
