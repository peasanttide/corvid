//! What one consumer remembers between two polls.

/// How much of a signal's history one consumer has already read.
///
/// It is a sequence number and nothing else -- eight bytes, `Copy`, and holding
/// no reference to the signal it was read against. A consumer keeps one per
/// [`Watch`](crate::Watch) it polls and passes it back by `&mut` every time.
///
/// [`Seen::default()`](Default::default) has seen **nothing**, so the first
/// [`changed_since`](crate::Watch::changed_since) against a fresh one reports
/// the value in the cell rather than [`None`]. That is what a consumer starting
/// up wants: the signal holds state, and a consumer that skipped the state
/// already there would render a window at the wrong size until the next resize.
/// A consumer that wants only what happens *after* it started asks the watch for
/// [`seen_now`](crate::Watch::seen_now) instead.
///
/// # What the caller owes
///
/// A `Seen` means something only against the `Watch` it was polled with. The
/// type system does not enforce that -- there is no lifetime or channel
/// parameter on it, deliberately, so that a consumer can keep an array of them
/// beside an array of watches. Poll a `Seen` against a different signal and it
/// answers about a sequence number that signal assigned to something else: a
/// change gets reported that never happened, or one that did happen is missed.
/// Nothing detects it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Seen(u64);

impl Seen {
    /// A `Seen` that has read up to `sequence`.
    pub(crate) const fn at(sequence: u64) -> Self {
        Self(sequence)
    }

    /// The sequence number this consumer last read.
    pub(crate) const fn sequence(self) -> u64 {
        self.0
    }
}
