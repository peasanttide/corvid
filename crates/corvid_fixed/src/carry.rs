//! Moving at a velocity, on average, when a position cannot hold the answer.
//!
//! # The problem
//!
//! A velocity times a time step is a displacement, and the displacement almost
//! never lands on a representable value. `I24F8` stores a position to about
//! 3.9 mm; a camera at 4 m/s on a 240 Hz display covers 16.7 mm in a frame,
//! which is 4.27 of those steps. Round each frame and 0.27 of a step is lost
//! **every frame** — the same 0.27, in the same direction, forever. That is 6%
//! of the speed, and it gets worse the faster the display runs, which is the
//! most confusing possible symptom: the game is slower on better hardware.
//!
//! Rounding to nearest does not fix it. It halves the constant for most
//! velocities and leaves it at its worst for exactly the ones that land on a
//! half step, and a velocity a player holds is a velocity that lands on the same
//! fraction every frame. **A per-step rounding rule cannot be right, because the
//! error is not random — it is a function of the velocity, and the velocity is
//! held.**
//!
//! # What this does instead
//!
//! Keeps what it could not pay. The exact displacement of a step is
//!
//! ```text
//! velocity_bits × microseconds / 1_000_000
//! ```
//!
//! and this computes that as an integer division, hands back the quotient, and
//! carries the **remainder** into the next step. The remainder is exact — it is
//! an integer of bit-microseconds, not a rounded fraction of anything — so no
//! error accumulates at all:
//!
//! > After any sequence of steps, the total displacement handed back differs
//! > from the exact total by less than one representable step, however many
//! > steps there were and whatever the velocities were.
//!
//! Not "less than one step per frame". Less than one step, total, forever.
//! `tests/carry.rs` runs a million frames at rates chosen to be maximally
//! unfriendly and asserts exactly that bound.
//!
//! It is the same idea as Bresenham's line algorithm and as error diffusion in
//! a dithered image, applied to time: never round, and never throw the
//! difference away.
//!
//! The division truncates towards zero rather than flooring, so the whole thing
//! is odd: negate the velocity and every intermediate negates, which means
//! moving one way is exactly moving the other way mirrored and a journey out and
//! back lands on the step it started from. Flooring bounds the total error just
//! as well and is *not* symmetric — it walks 69 steps one way and 70 the other
//! — which is an asymmetry no player could ever find and every long run would
//! carry.
//!
//! # What it is not
//!
//! Not smoothing, not interpolation, and not a physics integrator. Nothing here
//! remembers a position — the caller owns that — and nothing here is a filter:
//! the displacement it returns for a step is either the floor or the ceiling of
//! the exact one, never anything between and never anything outside.
//!
//! ```
//! use corvid_fixed::{Carry, I24F8};
//! use core::time::Duration;
//!
//! // Four metres a second, sampled at 240 Hz, into a position stored to 3.9 mm.
//! let velocity = I24F8::from_f64(4.0);
//! let frame = Duration::from_micros(4_166);
//! let mut carry = Carry::<I24F8>::ZERO;
//!
//! let mut travelled = 0i64;
//! for _ in 0..240 {
//!     travelled += i64::from(carry.step(velocity, frame).to_bits());
//! }
//!
//! // 240 frames of 4166 µs is 0.99984 s, so an exact answer is 3.99936 m —
//! // 1023.8 of the 1/256 m steps a position is stored in. Truncating each
//! // frame gives 960 of them: 6% short, and short by more on a faster display.
//! assert_eq!(travelled, 1_023);
//! ```

use core::marker::PhantomData;
use core::time::Duration;

/// The bit access every fixed-point type in this crate shares.
///
/// It exists so that [`Carry`] can be written once rather than five times, and
/// it deliberately says nothing about *units*: a velocity is "so many of this
/// type per second", and what the type means is the caller's. That is what lets
/// one implementation serve a position in metres, an angle a turret slews
/// through, and a factor a fade runs at.
///
/// Nothing here is about fractional bits, because the arithmetic does not need
/// them: a displacement in bits is a velocity in bits times a duration, and the
/// scale cancels.
pub trait Fixed: Copy {
    /// The integer this type is stored as.
    type Bits: Copy;

    /// The zero value.
    const ZERO: Self;

    /// The raw bit pattern.
    fn to_bits(self) -> Self::Bits;

    /// Wraps a raw bit pattern.
    fn from_bits(bits: Self::Bits) -> Self;

    /// The bit pattern, sign-extended to the widest integer there is.
    fn to_wide(self) -> i128;

    /// The value a wide integer denotes, saturating at this type's bounds.
    ///
    /// Saturating rather than wrapping **even for the modular families**. What
    /// goes through here is a displacement — how far something moved during one
    /// step — and a step that moved more than a whole turn is a velocity nobody
    /// can see the direction of, so pinning it is the honest answer and
    /// wrapping it would silently reverse it.
    fn from_wide(wide: i128) -> Self;
}

/// Microseconds in a second, which is the divisor every step performs.
const MICROS_PER_SECOND: i128 = 1_000_000;

/// What a velocity has been asked to move and has not yet moved.
///
/// One per axis: a camera flying in three dimensions holds three of these,
/// because the remainders are independent and folding them into one would let
/// motion along `x` pay a debt owed along `y`.
///
/// Hold it beside the position it moves — in a `View`, in a `State`, in a
/// particle — and hand it the velocity each step. It is `Copy`, hashable and
/// comparable, so a game that keeps one in its simulation state can, and one
/// that keeps it in a view costs nothing.
///
/// **A `Carry` in a hashed state is part of that state.** Two peers that agree
/// about a velocity and disagree about what it is owed will disagree about the
/// next step, so it has to travel with the position rather than be rebuilt —
/// which is exactly why it is `Data`-shaped rather than a private cache.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Carry<T> {
    /// Bit-microseconds owed, in `-1_000_000 .. 1_000_000`.
    ///
    /// **Signed, and it carries the sign of the direction being travelled.**
    /// The division truncates towards zero rather than flooring, which is the
    /// one decision in this type that a test had to settle rather than an
    /// argument: flooring bounds the total error just as well, and it is not
    /// *symmetric*. Ten thousand frames at one bit per second walk 69 steps
    /// forwards and 70 backwards under a floor, because a floor rounds down in
    /// both directions and "down" means towards the destination going one way
    /// and away from it going the other.
    ///
    /// Truncating towards zero makes the whole computation odd — negate the
    /// velocity and every intermediate negates — so moving left is exactly
    /// moving right mirrored, and a journey out and back lands on the step it
    /// started from. `the_two_directions_are_exactly_symmetric` is the test
    /// that says so, and it failed against the first version of this.
    owed: i64,
    /// What this carries for, which costs nothing at run time.
    kind: PhantomData<fn() -> T>,
}

// The derives are written out rather than derived, because `derive` on a type
// with a parameter adds a `T: Trait` bound and `T` is only a marker here — a
// `Carry<I24F8>` is `Copy` whatever `I24F8` is.
impl<T> Clone for Carry<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Carry<T> {}

impl<T> PartialEq for Carry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.owed == other.owed
    }
}

impl<T> Eq for Carry<T> {}

impl<T> core::hash::Hash for Carry<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.owed.hash(state);
    }
}

impl<T> Default for Carry<T> {
    fn default() -> Self {
        Self::ZERO
    }
}

impl<T> Carry<T> {
    /// Owing nothing, which is where anything that has not moved yet starts.
    pub const ZERO: Self = Self {
        owed: 0,
        kind: PhantomData,
    };

    /// How much of a step is owed and unpaid, in bit-microseconds.
    ///
    /// Between zero and a million. Worth reading only to assert something about
    /// it: it is the internal state of the rounding and not a quantity a game
    /// has any use for.
    #[must_use]
    #[inline]
    pub const fn owed(self) -> i64 {
        self.owed
    }

    /// Forgets what is owed.
    ///
    /// For a thing that has been teleported rather than moved: the debt belongs
    /// to a journey that is no longer happening, and carrying it across a cut
    /// spends it on the first step of the next one.
    #[inline]
    pub const fn reset(&mut self) {
        self.owed = 0;
    }
}

impl<T: Fixed> Carry<T> {
    /// How far `velocity` carries something in `dt`, with what could not be
    /// paid kept for next time.
    ///
    /// `velocity` is in units of `T` **per second**; the answer is in units of
    /// `T`. A step of zero duration moves nothing and owes nothing new, and a
    /// zero velocity pays nothing and forgets nothing — a carry that had a
    /// remainder when the player let go still has it when they press again,
    /// which is what makes tapping a direction cover the same ground as holding
    /// it.
    ///
    /// # Saturation
    ///
    /// The product is computed at 128 bits, so nothing overflows on the way in
    /// for any type this crate has and any duration a `Duration` can hold. The
    /// way *out* saturates: a displacement wider than `T` pins at its bound,
    /// which can only happen for a velocity and a step whose product leaves the
    /// type's range — a metre-per-second type asked to cross a light year.
    ///
    /// Unlike the trigonometry in this crate, this is not a hot loop: it runs
    /// once per axis per displayed frame, so 128-bit arithmetic here buys
    /// exactness for a cost nothing can measure.
    pub fn step(&mut self, velocity: T, dt: Duration) -> T {
        // `as_micros` is a `u128` and is the whole of the time this reads: a
        // step measured in milliseconds throws away 94% of a 144 Hz frame,
        // which this crate's callers have already been bitten by once.
        let micros = i128::try_from(dt.as_micros()).unwrap_or(i128::MAX);
        let exact = velocity
            .to_wide()
            .saturating_mul(micros)
            .saturating_add(i128::from(self.owed));

        // Towards zero, and the remainder keeps the sign. That is what makes
        // the two directions mirror each other exactly; see the `owed` field,
        // where the measurement that settled it is written down.
        let whole = exact / MICROS_PER_SECOND;
        let owed = exact % MICROS_PER_SECOND;

        // The remainder is below a million in magnitude by construction, so
        // this conversion cannot fail; `unwrap_or` rather than an unwrap
        // because the workspace denies the latter and a debt of zero is the
        // harmless answer.
        self.owed = i64::try_from(owed).unwrap_or(0);
        T::from_wide(whole)
    }

    /// The same, for something moving along several axes at once.
    ///
    /// One carry per axis, which is what the array is: the remainders are
    /// independent, and a single shared one would let motion along `x` pay a
    /// debt owed along `y` and bend a straight line.
    ///
    /// ```
    /// use corvid_fixed::{Carry, I24F8};
    /// use core::time::Duration;
    ///
    /// let mut carry = [Carry::<I24F8>::ZERO; 3];
    /// let velocity = [I24F8::from_f64(4.0), I24F8::ZERO, I24F8::from_f64(-1.0)];
    /// let step = Carry::step_each(&mut carry, velocity, Duration::from_micros(6_944));
    ///
    /// // The axis nobody is moving along stays exactly still, and the two that
    /// // are move in opposite directions by the ratio of their velocities.
    /// assert_eq!(step[1], I24F8::ZERO);
    /// assert!(step[0].to_bits() > 0 && step[2].to_bits() < 0);
    /// ```
    pub fn step_each<const N: usize>(
        carry: &mut [Self; N],
        velocity: [T; N],
        dt: Duration,
    ) -> [T; N] {
        let mut moved = [T::ZERO; N];
        for ((slot, axis), speed) in moved.iter_mut().zip(carry.iter_mut()).zip(velocity) {
            *slot = axis.step(speed, dt);
        }
        moved
    }
}
