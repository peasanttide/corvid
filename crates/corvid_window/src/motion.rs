//! Relative axes, and the only thing between the pixels a device reported and
//! the fraction of a sweep a frame is told about.
//!
//! A mouse reports the motion that already happened and a stick reports how far
//! it is pushed, and those are not the same kind of number.
//! [`Input::delta`](corvid_input::Input::delta) is the first and
//! [`Input::analog`](corvid_input::Input::analog) is the second, and everything
//! this module handles is the first: what arrives here is a displacement, what
//! leaves is a displacement, and nothing on the way multiplies or divides by a
//! frame time. That is what makes the sweep a player makes turn the camera by
//! the same amount however the display is behaving -- a frame that lasted twice
//! as long saw twice the pixels and is told about twice the fraction, and the
//! sum over a sweep is the sweep.
//!
//! What is left for this module to do is the ceiling. A binding's span is how
//! many device units make a full sweep, and a frame that saw more than a span
//! of them would be clamped by
//! [`Devices::snapshot`](corvid_input::platform::Devices::snapshot) -- which is
//! motion thrown away rather than deferred. So this holds what the device has
//! reported and not yet been handed over for, hands over at most a span of it
//! per frame, and carries the rest. What is emitted is subtracted from the debt
//! and nothing else is, so what a game integrates over a burst is the
//! displacement the device reported rather than whatever fitted in one frame.
//!
//! The one place motion is deliberately dropped is a debt older than [`LAG`]
//! frames' worth. A pointer warped across the screen is not a sweep, and an
//! axis left pinned while a debt like that is paid off would be a camera
//! turning on its own after the hand stopped -- which is a worse bug than the
//! clamp.
//!
//! # Where the frame rate does still show through, and how far
//!
//! The paragraph above about a sweep summing to the sweep holds *below the
//! ceiling*, and it is worth being exact about where that stops, because the
//! bound is denominated in frames rather than in time and a frame is not a
//! fixed amount of time.
//!
//! A frame can hand over at most one span, because the fraction it hands over
//! is an [`Analog`](corvid_input::Analog) and one of those cannot say "more
//! than a whole sweep". That part is the storage and no arithmetic here can
//! change it. What the debt adds is [`LAG`] more spans of backlog, and past
//! that motion is dropped -- so the most a run can deliver over `n` frames is
//! `(n + LAG)` spans however long those frames lasted. At 320 pixels a span
//! that is 19 200 px/s at 60 Hz and 4 800 px/s at 15 Hz: a flick fast enough to
//! exceed the second is clipped on a display slow enough to be the second,
//! and the same flick at 144 Hz is not. Ordinary aiming is nowhere near either
//! number, which is why the sweep tests below pass without touching a clamp.
//!
//! Denominating the backlog in frames rather than in time is the deliberate
//! part. A ceiling per *unit of time* would drain a long frame proportionally
//! more and so would not have that edge, but it costs an interval that has to
//! be measured, threaded down here and divided by, and a reference frame period
//! to scale the answer against. A clipped flick at fifteen frames a second is
//! what this module pays instead, and giving the backlog a time budget is what
//! it would take to buy the edge back.
//!
//! `examples/jitter` measures a displacement handled as one against the same
//! number handled as a rate, and `../README.md` has what it reports.

use corvid_input::platform::{Axis, Bindings, Devices, Reading};

/// How many frames' worth of full sweep a debt may be.
///
/// A device that reports a motion faster than a full sweep -- a teleport, a
/// tablet pen jumping across the screen, a virtual pointer being warped -- would
/// otherwise leave the axis pinned for as long as it takes to pay the debt off.
/// Two frames is the longest overshoot this crate will produce.
const LAG: i64 = 2;

/// What one relative axis owes, in the device's own units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Debt {
    /// Which axis this is the debt of.
    axis: Axis,
    /// What is owed along each of the two axes of that control.
    owed: [i64; 2],
}

/// What every relative axis has reported and not yet been handed over for.
#[derive(Clone, Debug, Default)]
pub(crate) struct Motion {
    /// One entry per axis the platform has ever reported. Two, on a desktop.
    debts: Vec<Debt>,
}

impl Motion {
    /// Nothing owed on any axis.
    pub(crate) const fn new() -> Self {
        Self { debts: Vec::new() }
    }

    /// Records that `axis` moved by `dx`, `dy` in the device's own units.
    ///
    /// Accumulated rather than replaced, because a device that reports four
    /// times between two frames moved the sum of them. Saturating, so a device
    /// that reports an absurd delta owes a great deal rather than wrapping into
    /// owing the opposite.
    pub(crate) fn moved(&mut self, axis: Axis, dx: i32, dy: i32) {
        if !self.debts.iter().any(|debt| debt.axis == axis) {
            self.debts.push(Debt { axis, owed: [0, 0] });
        }
        let Some(debt) = self.debts.iter_mut().find(|debt| debt.axis == axis) else {
            return;
        };
        for (owed, delta) in debt.owed.iter_mut().zip([dx, dy]) {
            *owed = owed.saturating_add(i64::from(delta));
        }
    }

    /// Forgets every debt.
    ///
    /// What a window does when it loses focus, for the same reason
    /// [`Devices::released_all`] drops the motion it had accumulated: motion
    /// that happened while another window had the pointer is not motion in this
    /// one, and paying it out afterwards would swing the camera on the frame
    /// the player came back.
    pub(crate) fn forget(&mut self) {
        self.debts.clear();
    }

    /// Hands every axis as much of its debt as one frame can carry.
    ///
    /// Call this once per frame, immediately before
    /// [`Devices::snapshot`](corvid_input::platform::Devices::snapshot), which
    /// is what turns the displacement into a fraction of a sweep and then
    /// clears it.
    ///
    /// An axis that owes nothing and is paying nothing is left out rather than
    /// handed a pair of zeroes, because `Devices` keeps its accumulated motion
    /// in a map it clears every snapshot: an entry that says zero is a node
    /// allocated and freed once a frame to say what its absence already says.
    pub(crate) fn pay(&mut self, bindings: &Bindings, devices: &mut Devices) {
        for debt in &mut self.debts {
            let ceiling = ceiling(debt.axis, bindings);
            let x = pay(&mut debt.owed[0], ceiling);
            let y = pay(&mut debt.owed[1], ceiling);
            if (x, y) != (0, 0) {
                devices.moved(debt.axis, x, y);
            }
        }
    }
}

/// The largest displacement an axis can be handed in one frame without a
/// binding clamping it.
///
/// A reading is the displacement divided by the binding's span, so a
/// displacement above the span is one the snapshot would clamp -- and a clamp is
/// motion thrown away rather than deferred, which is the thing this module is
/// for.
///
/// The *smallest* span wins where an axis drives several actions, because it is
/// the one that clamps first. That costs the other actions their top end: an
/// action bound at a span of a thousand on an axis another action binds at a
/// hundred never reads past a tenth of a sweep. Nothing in this workspace binds
/// one axis twice, and a per-binding ceiling would need a per-binding value to
/// hand over, which [`Devices`] has no way to take.
///
/// Only [`Reading::Displacement`] bindings count. A [`Reading::Deflection`]
/// binding on the same control is served from a different map -- `Devices` keeps
/// levels apart from motion -- and never reads a byte of this debt, so letting
/// its span into the minimum would let a stick-shaped binding throttle a mouse
/// that has nothing to do with it. It is a units error as much as a bug: a
/// stick's span and a mouse's pixels are not the same quantity, and `min` over
/// the two compares numbers that do not mean the same thing.
///
/// An axis nothing binds *this way* gets the widest ceiling there is, since no
/// snapshot reads the debt and the only thing a narrow one would do is round it
/// away.
fn ceiling(axis: Axis, bindings: &Bindings) -> i64 {
    bindings
        .axes()
        .iter()
        .filter(|binding| binding.axis == axis && binding.reading == Reading::Displacement)
        .map(|binding| i64::from(binding.span.get().min(i32::MAX.unsigned_abs())))
        .min()
        .unwrap_or_else(|| i64::from(i32::MAX))
}

/// How much of one axis of one control this frame is handed, and what that
/// leaves owed.
///
/// The answer is in the device's own units, which is what a span is measured
/// in. Everything it emits it subtracts, so the emitted displacements summed
/// over the frames they were emitted for are the displacement that was put in --
/// the clamp defers rather than discards.
fn pay(owed: &mut i64, ceiling: i64) -> i32 {
    *owed = (*owed).clamp(ceiling.saturating_mul(-LAG), ceiling.saturating_mul(LAG));
    let paid = (*owed).clamp(-ceiling, ceiling);
    *owed -= paid;
    i32::try_from(paid).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    //! What the debt does that handing the raw displacement over does not.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::{LAG, Motion, pay};
    use corvid_input::platform::Axis;

    /// A span, in device units, wide enough that ordinary motion is nowhere
    /// near it -- the placeholder table's own.
    const SPAN: i64 = 320;

    #[test]
    fn an_ordinary_frame_hands_over_exactly_the_pixels_it_saw() {
        // Nothing is scaled and nothing is deferred while the frame is inside
        // the ceiling, which is every frame of ordinary play.
        let mut owed = 8;
        assert_eq!(pay(&mut owed, SPAN), 8);
        assert_eq!(owed, 0);
    }

    #[test]
    fn the_same_sweep_hands_over_the_same_total_at_every_frame_rate() {
        // The bug, stated as a test. One second of real time and one sweep of
        // the hand, at five frame rates from fifteen a second to a hundred and
        // seventy-six thousand -- the two ends of what `examples/jitter`
        // measures on a real display. What a consumer sums is what is handed
        // over, and that is what has to agree.
        //
        // The pixels are spread over the frames as evenly as integers allow,
        // which is what lets a rate that does not divide the sweep be one of
        // the rows: a thousand frames carrying six hundred pixels is six
        // hundred frames of one pixel and four hundred of none, which is also
        // what a 125 Hz mouse under a 1 kHz loop actually looks like.
        let sweep = |frames: i64, pixels: i64| -> i64 {
            let mut owed = 0;
            let mut sent = 0;
            let mut handed = 0;
            for frame in 1..=frames {
                let so_far = pixels * frame / frames;
                owed += so_far - sent;
                sent = so_far;
                handed += i64::from(pay(&mut owed, SPAN));
            }
            assert_eq!(sent, pixels, "the fixture did not send the whole sweep");
            // Still owed at the end, so the comparison is of what was
            // delivered plus what is about to be.
            handed + owed
        };
        assert_eq!(sweep(15, 600), 600);
        assert_eq!(sweep(60, 600), 600);
        assert_eq!(sweep(600, 600), 600);
        assert_eq!(sweep(1_000, 600), 600);
        assert_eq!(sweep(176_000, 600), 600);
        // And the control, because "every rate agrees" would also hold of an
        // axis that reported nothing at all: half the sweep is half the total.
        assert_eq!(sweep(60, 300), 300);
    }

    #[test]
    fn a_burst_too_fast_for_one_frame_is_paid_over_the_next_ones() {
        // Five hundred pixels between two frames is more than a span, which is
        // as far as one snapshot can express. Handing the raw number over would
        // clamp and lose the rest; this defers it.
        let mut owed = 500;
        let mut delivered = 0;
        for _ in 0..4 {
            delivered += i64::from(pay(&mut owed, SPAN));
        }
        assert_eq!(delivered, 500);
        assert_eq!(owed, 0);
        // And the first frame really was capped, which is the half that says
        // the deferral happened at all rather than the whole burst going
        // through in one frame.
        let mut again = 500;
        assert_eq!(pay(&mut again, SPAN), i32::try_from(SPAN).unwrap_or(0));
    }

    #[test]
    fn nothing_is_owed_for_longer_than_the_lag_allows() {
        // A pointer warped across the screen is not a sweep, and an axis left
        // pinned while a debt like that is paid off would be a camera turning
        // on its own. This is the one place motion is deliberately dropped.
        let mut owed = 1_000_000;
        assert_eq!(pay(&mut owed, SPAN), i32::try_from(SPAN).unwrap_or(0));
        assert!(
            owed <= SPAN * LAG,
            "a warp left {owed} owed, which is more than the ceiling allows",
        );
    }

    #[test]
    fn motion_is_summed_per_axis_and_the_axes_do_not_mix() {
        let mut motion = Motion::new();
        motion.moved(Axis::MouseMotion, 3, -2);
        motion.moved(Axis::MouseMotion, 4, 0);
        motion.moved(Axis::Scroll, 1, 1);
        let mouse = motion
            .debts
            .iter()
            .find(|debt| debt.axis == Axis::MouseMotion)
            .map(|debt| debt.owed);
        assert_eq!(mouse, Some([7, -2]));
        motion.forget();
        assert!(motion.debts.is_empty());
    }
}
