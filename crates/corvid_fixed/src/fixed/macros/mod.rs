//! Code generators shared by the five families.
//!
//! Each family module ([`point`](super::point), [`factor`](super::factor),
//! [`signed`](super::signed), [`angle`](super::angle),
//! [`pitch`](super::pitch)) owns the macro that knows its arithmetic, and calls
//! into this module for everything the families have in common: the newtype
//! declaration with its optional derives, bit access, comparison, formatting,
//! and the `num-traits` glue.
//!
//! The generated types are separate structs rather than aliases of one generic
//! type. That keeps the families from mixing -- a [`Factor16`](crate::Factor16)
//! cannot be added to an [`Angle16`](crate::Angle16) -- and keeps rustdoc showing
//! concrete signatures.
//!
//! # Contract
//!
//! A family macro must define these before invoking [`impl_shared`]:
//!
//! - `MIN` and `MAX` associated constants.
//! - `const fn cmp_key(self) -> $repr`, the canonical bit pattern, used for
//!   equality, ordering, hashing, and the results of `min`, `max`, and `clamp`.
//!   It is the identity for the fixed-point, factor, and angle families. The
//!   signed-normalized family folds its denormal encoding of `-1.0`; the pitch
//!   family clamps bit patterns lying outside `MIN ..= MAX`.
//! - `const fn to_f64(self) -> f64`, and the `from_f64` / `checked_from_f64`
//!   pair that the `f32` conversions here are defined in terms of.
mod num_traits;
mod shared;

pub(super) use num_traits::{
    impl_num_traits_arith, impl_num_traits_shared, impl_num_traits_wrapping,
};
pub(super) use shared::{define_newtype, impl_binop, impl_neg, impl_one, impl_shared};
