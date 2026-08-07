//! Which cues have already been played, and what that cannot tell you.

use corvid_sound::CueId;

/// The identities of the cues this backend has already started.
///
/// # Why a backend needs one at all
///
/// A cue is fired by the simulation and read by the client once per *displayed*
/// frame, so a fifteen-hertz tick is read nine or ten times over. Without a
/// record of what has been started, the bounce on tick 97 would be played ten
/// times — once per frame that could still see it.
///
/// A [`CueId`] is what makes that record possible, and `corvid_sound` explains
/// at length why it has to be disjoint from the payload: the same cue's
/// position and gain change between two readings of one tick, because the
/// listener moved.
///
/// # What rollback does to it, and what this decides
///
/// A rollback re-simulates ticks that have already been read, and sound does
/// not rewind: the thud has left the speaker. So an identity can **disappear**
/// from the frame after it was played, and one that was played can
/// **reappear**, possibly carrying a different payload. This type takes a
/// position on all three cases, and each position is a decision rather than a
/// consequence:
///
/// A cue that **disappears** is left to ring out. The alternative is cutting a
/// voice that is already sounding, and the sounds this backend makes are short
/// percussive knocks where a cut is a click and a ring-out is a sound nobody
/// notices was wrong. That reasoning does not survive a sound with a long tail,
/// and a backend playing recorded music would want to duck it instead.
///
/// A cue that **reappears** is not restarted, because its identity is
/// remembered. If the re-simulation gave it a different gain or a different
/// pitch, the voice already playing keeps the old one — retuning a percussive
/// one-shot part way through its decay is a click, and this backend has nothing
/// to retune it with anyway.
///
/// A cue whose identity has been **forgotten** is treated as new and played
/// again. That is the cost of a bounded memory and it is why
/// [`remembers`](Self::remembers) is a number a caller chooses: the ring holds
/// the last N identities started, and a rollback reaching back further than N
/// cues will replay what it finds.
///
/// # What this does not decide
///
/// It does not decide when a cue is too old to be worth playing. A frame
/// extracted after a long stall carries every cue of the tick it was extracted
/// from, and a backend that started all of them at once would fire a burst of
/// sounds for events the player has already walked past. This one plays them,
/// and a mixer that cared would need a tick and a rate to compare against —
/// neither of which is in an [`AudioFrame`](corvid_sound::AudioFrame).
///
/// It does not decide anything about [`Source`](corvid_sound::Source)s, which
/// are voices held open across frames rather than events, and which this crate
/// does not play at all.
///
/// ```
/// use corvid_audio::Heard;
/// use corvid_sound::CueId;
/// use corvid_time::Tick;
///
/// let mut heard = Heard::new(16);
/// let bounce = CueId::new(Tick(97), 0);
///
/// // The first reading of tick 97 starts it; the next nine do not.
/// assert!(heard.is_new(bounce));
/// assert!(!heard.is_new(bounce));
///
/// // A second bounce on the same tick is a second cue.
/// assert!(heard.is_new(CueId::new(Tick(97), 1)));
///
/// // And a rollback that un-fires the first and fires it again does not
/// // play it twice.
/// assert!(!heard.is_new(bounce));
/// ```
#[derive(Clone, Debug)]
pub struct Heard {
    /// The last identities started, oldest overwritten first.
    ring: Box<[Option<CueId>]>,
    /// Where the next identity goes.
    next: usize,
}

impl Heard {
    /// A record with room for `remembers` identities.
    ///
    /// At least one, because a record that remembers nothing calls every
    /// reading of one cue a new cue and plays a tick's worth of bounces once
    /// per displayed frame.
    #[must_use]
    pub fn new(remembers: usize) -> Self {
        Self {
            ring: vec![None; remembers.max(1)].into_boxed_slice(),
            next: 0,
        }
    }

    /// How many identities this remembers.
    #[must_use]
    pub const fn remembers(&self) -> usize {
        self.ring.len()
    }

    /// Whether `id` has not been started, recording it if so.
    ///
    /// The recording is the point: this is asked once per cue per displayed
    /// frame and the answer has to be true exactly once.
    pub fn is_new(&mut self, id: CueId) -> bool {
        if self.ring.contains(&Some(id)) {
            return false;
        }
        if let Some(slot) = self.ring.get_mut(self.next) {
            *slot = Some(id);
        }
        self.next = (self.next + 1) % self.ring.len().max(1);
        true
    }

    /// Forgets everything, so every identity is new again.
    ///
    /// What a backend does when a session is replaced — a load, a new match —
    /// where the same ticks are about to be simulated again and their cues are
    /// genuinely new sounds rather than the ones already played.
    pub fn forget_all(&mut self) {
        for slot in &mut *self.ring {
            *slot = None;
        }
        self.next = 0;
    }
}

#[cfg(test)]
mod tests {
    //! The three cases a rollback produces, and the one the ring gives up on.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::Heard;
    use corvid_sound::CueId;
    use corvid_time::Tick;
    #[test]
    fn one_cue_read_ten_times_is_started_once() {
        // Ten displayed frames over one fifteen-hertz tick, which is what a
        // hundred-and-fifty-hertz display does.
        let mut heard = Heard::new(64);
        let bounce = CueId::new(Tick(97), 0);
        let started = (0..10).filter(|_| heard.is_new(bounce)).count();
        assert_eq!(started, 1);
    }

    #[test]
    fn two_cues_on_one_tick_are_two_sounds() {
        let mut heard = Heard::new(64);
        assert!(heard.is_new(CueId::new(Tick(97), 0)));
        assert!(heard.is_new(CueId::new(Tick(97), 1)));
        // And the same serial on the next tick is a third.
        assert!(heard.is_new(CueId::new(Tick(98), 0)));
    }

    #[test]
    fn a_cue_that_disappears_and_comes_back_is_not_played_twice() {
        // The rollback case. Tick 97's bounce is read, a correction removes it
        // for three frames, and the re-simulation puts it back.
        let mut heard = Heard::new(64);
        let bounce = CueId::new(Tick(97), 0);
        assert!(heard.is_new(bounce));
        // Three frames in which the bounce is not in the frame at all, and
        // something else is.
        assert!(heard.is_new(CueId::new(Tick(98), 0)));
        assert!(!heard.is_new(CueId::new(Tick(98), 0)));
        assert!(!heard.is_new(CueId::new(Tick(98), 0)));
        assert!(!heard.is_new(bounce));
    }

    #[test]
    fn an_identity_older_than_the_ring_is_played_again() {
        // The cost of a bounded memory, asserted rather than hoped for: a
        // caller choosing a size is choosing how far back a rollback may reach
        // before it replays something.
        let mut heard = Heard::new(4);
        let first = CueId::new(Tick(1), 0);
        assert!(heard.is_new(first));
        for serial in 0..4 {
            assert!(heard.is_new(CueId::new(Tick(2), serial)));
        }
        assert!(
            heard.is_new(first),
            "the ring kept more than it has room for"
        );
    }

    #[test]
    fn a_ring_that_was_asked_for_nothing_still_remembers_one() {
        // Zero would be a modulus of zero and a record that answers new to
        // everything, which plays a tick's cues once per displayed frame.
        let mut heard = Heard::new(0);
        assert_eq!(heard.remembers(), 1);
        let bounce = CueId::new(Tick(97), 0);
        assert!(heard.is_new(bounce));
        assert!(!heard.is_new(bounce));
    }

    #[test]
    fn forgetting_makes_every_identity_new_again() {
        let mut heard = Heard::new(64);
        let bounce = CueId::new(Tick(97), 0);
        assert!(heard.is_new(bounce));
        heard.forget_all();
        assert!(heard.is_new(bounce));
    }
}
