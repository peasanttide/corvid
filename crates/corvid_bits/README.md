# `corvid_bits`

Bit manipulation for a [Corvid](https://github.com/peasanttide/corvid) game.
`no_std`, no dependencies, every function `const`.

Two questions, both of which the rest of the workspace was asking at each site
in its own words.

**How wide is this magnitude?** Reciprocal square roots, quaternion
normalization and world-scale distances all begin by working out how many bits
a value occupies so they can shift it into the range their fixed-point kernel
is fitted to. That is `bit_length`.

```rust
use corvid_bits::bit_length_u64;

assert_eq!(bit_length_u64(0), 0);
assert_eq!(bit_length_u64(1), 1);
assert_eq!(bit_length_u64(255), 8);
assert_eq!(bit_length_u64(256), 9);
```

**Does this wide intermediate still fit?** A fixed-point multiply widens to
`i64` or `i128` and has to come back to the `i32` a component is. Two answers
are useful and this crate has both: saturate, for a quantity where the extreme
is the honest answer, and `Option`, for one where it is not.

```rust
use corvid_bits::{narrow_i64, try_narrow_i64};

assert_eq!(narrow_i64(1 << 40), i32::MAX);
assert_eq!(try_narrow_i64(1 << 40), None);
assert_eq!(try_narrow_i64(-5), Some(-5));
```

## Why a crate

Both were written out at the point of use, and the arithmetic they belong to is
not always the arithmetic they were written in. `bit_length` appeared eight
times across four crates and the narrowing six times with four different
signatures, which is six chances to get a boundary wrong and six sets of tests
that each cover it a little differently.

Being a leaf with no dependencies is what lets `corvid_fixed` -- the crate
everything else here is built on -- depend on it.
