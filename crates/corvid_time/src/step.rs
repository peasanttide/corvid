//! The fixed step: real time in, whole ticks out.

use core::time::Duration;

use corvid_fixed::Factor16;

use crate::TickSpan;

/// The most ticks one [`advance`](Step::advance) may return unless told
/// otherwise. A ceiling and not a quota: an advance that owes fewer delivers
/// fewer, and an advance of nothing delivers nothing.
///
/// Eight is half a second at fifteen hertz. A frame that overruns its budget
/// overruns it by a frame or two, and eight is generous room to make that back;
/// a gap wider than half a second is a load, a breakpoint, or a laptop lid, and
/// none of those are made better by simulating half a second of a game nobody
/// was watching.
const DEFAULT_CATCHUP: u32 = 8;

/// Turns elapsed real time into a whole number of ticks.
///
/// The step owns one integer accumulator measured in nanoseconds. Time handed
/// to [`advance`](Step::advance) is added to it, whole periods are taken out,
/// and the remainder stays for next time -- so a period split across ten calls
/// is still one tick, and a thousand exact periods are a thousand ticks with
/// nothing accumulated and nothing lost. There is no floating point in any of
/// it, [`alpha`](Step::alpha) included.
///
/// ```
/// use core::time::Duration;
/// use corvid_time::{Step, TickSpan};
///
/// let rate = TickSpan::CRADLE;
/// let mut step = Step::new(rate);
/// let mut ticks = 0;
/// for _ in 0..1000 {
///     ticks += step.advance(rate.period());
/// }
/// assert_eq!(ticks, 1000);
/// assert_eq!(step.dropped(), 0);
/// ```
///
/// Deliberately not `Copy`, for the reason [`Clock`](crate::Clock) sets out: a
/// step is an accumulator, and a copy that gets advanced is time the original
/// never hears about.
// No `Hash`, for the reason `Clock` has none: what a step holds is how far
// behind this machine is, and `corvid_hash::digest` would have taken it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The span this step was built from, kept so the caller can ask.
    span: TickSpan,
    /// `span.nanos()`, cached because it is a divisor on every advance.
    /// Never zero, which is what makes the division total.
    period_nanos: u64,
    /// Real time seen but not yet spent on a tick. Always below `period_nanos`
    /// once `advance` has returned.
    accumulated_nanos: u64,
    /// The most ticks one `advance` may return. Never zero.
    catchup: u32,
    /// Ticks that were owed and refused, counted since the step was built.
    dropped: u64,
}

impl Step {
    /// A step at `span`, with the default catch-up ceiling of eight ticks.
    #[must_use]
    #[inline]
    pub const fn new(span: TickSpan) -> Self {
        Self {
            span,
            period_nanos: span.nanos() as u64,
            accumulated_nanos: 0,
            catchup: DEFAULT_CATCHUP,
            dropped: 0,
        }
    }

    /// Sets the most ticks one [`advance`](Step::advance) may return.
    ///
    /// A ceiling of zero would mean a simulation that never ticks, so it is
    /// raised to one.
    #[must_use]
    #[inline]
    pub const fn with_catchup(mut self, max: u32) -> Self {
        self.catchup = if max == 0 { 1 } else { max };
        self
    }

    /// The span this step runs at.
    #[must_use]
    #[inline]
    pub const fn span(&self) -> TickSpan {
        self.span
    }

    /// The most ticks one [`advance`](Step::advance) will return.
    #[must_use]
    #[inline]
    pub const fn catchup(&self) -> u32 {
        self.catchup
    }

    /// How many ticks are owed after `elapsed` of real time.
    ///
    /// Whatever the catch-up ceiling refuses is *dropped*, not banked, and
    /// counted in [`dropped`](Step::dropped). That is the whole design of this
    /// type. A step that banked its backlog would hand a stalled process a
    /// thousand owed ticks, which take longer to simulate than the stall took
    /// to happen, which leaves more owed at the end than at the start -- a
    /// process that pauses for ten seconds would never catch up and never
    /// recover. Dropping loses simulated time that nobody watched, and the next
    /// second after a stall is an ordinary second.
    ///
    /// The remainder below one period survives the call, so the ticks that
    /// *are* delivered are on the same schedule they would have been without
    /// the stall, and [`alpha`](Step::alpha) picks up where it left off.
    ///
    /// A `Duration` counts past the range of a nanosecond counter, so an
    /// absurd one saturates rather than wrapping into a small number of ticks.
    #[inline]
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.accumulated_nanos = self.accumulated_nanos.saturating_add(nanos);

        let owed = self.accumulated_nanos / self.period_nanos;
        self.accumulated_nanos %= self.period_nanos;

        let delivered = owed.min(u64::from(self.catchup));
        self.dropped = self.dropped.saturating_add(owed - delivered);
        u32::try_from(delivered).unwrap_or(self.catchup)
    }

    /// Where the display sits between the last tick and the next.
    ///
    /// Zero immediately after a tick, and climbing toward one as the next comes
    /// due. A renderer interpolates the two states it was handed by this much,
    /// which is what lets a fifteen-hertz simulation drive a
    /// hundred-and-forty-four-hertz display without the picture stepping.
    ///
    /// It is a ratio of two integers -- nanoseconds accumulated over nanoseconds
    /// in a period -- rounded once onto a [`Factor16`], and never a fraction
    /// computed in binary floating point. Interpolation is not hashed, so the
    /// determinism argument does not apply here; the argument that does is that
    /// a sixteen-bit factor is what a renderer already carries at both ends, so
    /// computing the fraction any other way would only add a conversion to each
    /// of them and a rounding on the way through.
    ///
    /// The rounding means alpha reaches one within half of a factor's step of
    /// the next tick rather than only at it. At fifteen hertz that is the last
    /// five hundred nanoseconds of a sixty-six millisecond period, which no
    /// display can show.
    #[must_use]
    #[inline]
    pub fn alpha(&self) -> Factor16 {
        let scale = u64::from(Factor16::ONE.to_bits());
        // Round half up, matching how the rest of `corvid_fixed` rounds, with
        // the doubling done inside the numerator so the whole thing stays in
        // integers.
        //
        // It stays in a `u64`, and `TickSpan` is what makes that true rather
        // than an assumption about how anyone configures a game. A span is a
        // `u32` of nanoseconds and `advance` leaves the accumulator below one
        // span, so the numerator is under two times `u32::MAX` times 65 535 --
        // five hundred and sixty-three trillion against a ceiling of eighteen
        // quintillion, four orders of magnitude of room. The bound is in the
        // type, so neither a caller nor a save file can get underneath it.
        let numerator = 2 * self.accumulated_nanos * scale + self.period_nanos;
        let bits = numerator / (2 * self.period_nanos);
        // Exact by the same argument: the accumulator below one period makes
        // the quotient at most `scale`, which is `u16::MAX`. `unwrap_or` rather
        // than an unreachable branch, because a panic here would be a panic on
        // the interpolation path.
        Factor16::from_bits(u16::try_from(bits).unwrap_or(u16::MAX))
    }

    /// How many ticks the catch-up ceiling has refused since this step was
    /// built.
    ///
    /// Worth logging and worth surfacing: a run that drops ticks is a run whose
    /// simulated time no longer matches the wall clock, which is invisible from
    /// inside the simulation and obvious from here.
    #[must_use]
    #[inline]
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}
