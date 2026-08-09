//! The `nalgebra` integration.
//!
//! `nalgebra` is the float linear algebra this workspace converts to at the
//! device boundary, so what is checked is that a scalar survives the trip out
//! and back with the value it started with.

#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::panic_in_result_fn,
    reason = "these tests use ? for the library calls and assert! for the checks"
)]

use corvid_fixed::{Angle16, I24F8, Signed16};
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
