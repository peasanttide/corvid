//! The cadence a caller is not allowed to have yet.
//!
//! This is the single most legible thing the composer does, and it is one idea:
//! a phrase closes at its last bar, and while the tension a caller reports is
//! still rising it does not get there. The penultimate bar repeats under a
//! deceptive chord and the phrase's start slides forward, so the next bar is the
//! penultimate one again. The music will not let you go, and then it does.

use alloc::vec::Vec;

use crate::compose::Cadence;
use crate::compose::phrase::Phrase;

/// How many bars of tension the slope is read over.
const WINDOW: usize = 4;

/// How many times a cadence may be refused before it lands anyway.
///
/// Eight. Long enough that a listener notices being held, short enough that a
/// game whose tension never stops rising still gets a phrase that ends.
pub(crate) const MAX_DEFERRALS: u8 = 8;

/// The last few bars' tension, and how many cadences it has cost.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Tension {
    window: Vec<f32>,
    current: f32,
    deferrals: u8,
}

impl Tension {
    /// Sets what the caller says the tension is now.
    pub(crate) const fn set(&mut self, tension: f32) {
        self.current = tension;
    }

    /// Records the current tension as this bar's, dropping the oldest.
    pub(crate) fn remember(&mut self) {
        self.window.push(self.current);
        while self.window.len() > WINDOW {
            self.window.remove(0);
        }
    }

    /// Whether the window still has a positive slope.
    ///
    /// Read as the last against the first rather than as a fitted line: what
    /// matters is whether the crowd is angrier than it was, and a regression
    /// over four points would answer the same question with more arithmetic and
    /// one more thing to explain.
    pub(crate) fn rising(&self) -> bool {
        match (self.window.first(), self.window.last()) {
            (Some(first), Some(last)) if self.window.len() >= 2 => last - first > 1e-4,
            _ => false,
        }
    }

    /// How many times the current cadence has been refused.
    pub(crate) const fn deferrals(&self) -> u8 {
        self.deferrals
    }

    /// Forgets the deferrals, which a new phrase does.
    pub(crate) const fn release(&mut self) {
        self.deferrals = 0;
    }

    /// Whether a cadence lands on this bar, and which chord it is written on.
    ///
    /// The two answers are different questions. The first is what the bar
    /// *reports* -- and a deferred bar reports none, which is how a caller can
    /// see that the music is refusing to close. The second is what the harmony
    /// is made to do, which on a deferred bar is a deceptive chord and on the
    /// penultimate bar of a phrase that is allowed to end is the dominant.
    pub(crate) fn cadence_for(
        &mut self,
        position: u32,
        phrase: &mut Phrase,
    ) -> (Option<Cadence>, Option<Cadence>) {
        if phrase.length >= 2 && position + 2 == phrase.length {
            if self.rising() && self.deferrals < MAX_DEFERRALS {
                self.deferrals = self.deferrals.saturating_add(1);
                phrase.start = phrase.start.saturating_add(1);
                return (None, Some(Cadence::Deceptive));
            }
            return (None, Some(Cadence::Half));
        }
        if position + 1 >= phrase.length {
            self.deferrals = 0;
            return (Some(Cadence::Authentic), Some(Cadence::Authentic));
        }
        (None, None)
    }
}
