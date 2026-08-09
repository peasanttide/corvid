# `corvid_bits`

The two integer questions the rest of Corvid's arithmetic keeps asking. `no_std`,
no dependencies, every function `const`.

```rust
use corvid_bits::{bit_length_u64, narrow_i64, try_narrow_i64};

// How wide is this magnitude? A reciprocal square root, a quaternion
// normalization and a world-scale distance all start by asking, so they can
// shift a value into the range their kernel is fitted to.
assert_eq!(bit_length_u64(0), 0);
assert_eq!(bit_length_u64(255), 8);
assert_eq!(bit_length_u64(256), 9);

// Does this wide intermediate still fit? Saturating, where the extreme is the
// honest answer, and `Option` where it is not.
assert_eq!(narrow_i64(1 << 40), i32::MAX);
assert_eq!(try_narrow_i64(1 << 40), None);
assert_eq!(try_narrow_i64(-5), Some(-5));
```

A magnitude's width is [`bit_length_u32`], [`bit_length_u64`] and
[`bit_length_u128`] for an unsigned value, and [`magnitude_bits_i32`],
[`magnitude_bits_i64`] and [`magnitude_bits_i128`] for a signed one. The signed
form takes the magnitude through `unsigned_abs` rather than through a negation,
so [`i32::MIN`] answers 32 instead of overflowing.

Narrowing is [`narrow_i64`] and [`narrow_i128`] to saturate, or [`try_narrow_i64`]
and [`try_narrow_i128`] to be told. Both land on `i32`, which is the width a
fixed-point component comes back to after a multiply has widened it.

## Scope

Two questions, and a third arrives when a third question starts repeating across
crates rather than when one crate finds a use for it. A bit trick one crate needs
stays in that crate; what earns a place here is a boundary more than one of them
would otherwise get wrong separately.

Not a general bit-manipulation library: `count_ones`, `leading_zeros` and
`rotate_left` are already on the primitive integers, and nothing is copied here
to keep them company. Nothing here knows about fixed point either. These are
integer answers, and the crates above decide what the integers mean.
