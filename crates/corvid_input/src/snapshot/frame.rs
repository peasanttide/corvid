//! What happens to a snapshot between one tick and the next.
//!
//! Split from [`Input`](super::Input) because a file stays under 400 lines, and
//! this is the seam that was already there: everything in the parent answers a
//! question about the frame in hand, and everything here moves the frame along.
//! The three that matter are one cycle -- [`absorb`](Input::absorb) folds a
//! device reading in, [`settle`](Input::settle) ends the interval the edges
//! were accumulating over, and [`clear`](Input::clear) starts again from
//! nothing -- and they have to agree with each other about what an edge is,
//! which is easier to check when they are next to each other. The typed text
//! is here for the same reason: it accumulates over exactly that interval.

use super::Input;
use crate::value::{Analog, Digital};

impl Input {
    /// Folds a freshly read snapshot into this one, keeping whatever has not
    /// been spent yet.
    ///
    /// **Levels** -- `held`, the deflections, the poses, the pointer -- say what
    /// the device is doing *now*, so `fresh` replaces them outright.
    /// **Events** -- `pressed`, `released` and the displacements -- describe an
    /// interval, so they add up: the result is every edge and every millimetre
    /// since the last [`settle`](Self::settle), not just the ones in the last
    /// reading.
    ///
    /// This is what a loop that reads its devices more often than it ticks
    /// needs, and there is no such loop that does not. A window ends the edge
    /// interval once per displayed frame; at a fifteen-hertz tick on a
    /// sixty-hertz display three frames in four owe no tick at all, so a
    /// snapshot that was replaced rather than folded would drop the `pressed`
    /// of any tap that started and finished between two ticks -- "exactly the
    /// event a game must not miss", as [`Digital`] puts it -- along with
    /// three-quarters of the mouse.
    ///
    /// Displacements saturate rather than wrap, so an interval nobody ticked
    /// for a long time reads as a full sweep rather than as a sweep the other
    /// way.
    ///
    /// Values `fresh` has no slot for are left alone, which is what a snapshot
    /// over a shorter table amounts to.
    pub fn absorb(&mut self, fresh: &Self) {
        for (mine, theirs) in self.digital.iter_mut().zip(&fresh.digital) {
            mine.held = theirs.held;
            mine.pressed |= theirs.pressed;
            mine.released |= theirs.released;
        }
        for (mine, theirs) in self.analog.iter_mut().zip(&fresh.analog) {
            *mine = *theirs;
        }
        for (mine, theirs) in self.delta.iter_mut().zip(&fresh.delta) {
            // The scalar's own saturating add rather than one on the raw bits.
            // `Signed16` is `SNORM` and spends one pattern twice, so an
            // `i16::saturating_add` stops at `i16::MIN` -- the denormal
            // encoding of `-1.0`, one step outside what the type means. Its own
            // stops at `-1.0` and folds both operands on the way in.
            *mine = Analog::new(
                mine.x.saturating_add(theirs.x),
                mine.y.saturating_add(theirs.y),
            );
        }
        for (mine, theirs) in self.poses.iter_mut().zip(&fresh.poses) {
            *mine = *theirs;
        }
        // A level with two edges, folded exactly as a digital action is: the
        // freshest level wins and neither edge is lost, so a player who
        // alt-tabbed away and back between two ticks is reported as having done
        // both rather than as never having left.
        self.focus.held = fresh.focus.held;
        self.focus.pressed |= fresh.focus.pressed;
        self.focus.released |= fresh.focus.released;
        self.pointer = fresh.pointer;
        // An edge, like `pressed`: the first control of the interval is kept,
        // so a press that happened between two ticks is not dropped by a later
        // reading that saw nothing. `settle` is what spends it.
        self.captured = self.captured.or(fresh.captured);
        // An interval, like the edges above: what was typed over the whole
        // stretch since the last `settle`, in the order it was typed. A
        // snapshot that replaced this rather than appending would drop three
        // keystrokes in four at fifteen hertz on a sixty-hertz display.
        self.text.push_str(&fresh.text);
        // A level, like the deflections: the pointer is still in whatever mode
        // it is in, so the freshest reading is the whole answer.
        self.cursor = fresh.cursor;
        // And so is the window's size -- a resize is a state the display is in
        // rather than an interval something happened over.
        self.viewport = fresh.viewport;
    }

    /// Ends the interval the edges and the displacements describe, spending
    /// them.
    ///
    /// `pressed`, `released`, [`captured`](Self::captured), the two edges of
    /// [`focus`](Self::focus) and every displacement go to nothing. `held`, the
    /// deflections, the poses, the pointer, the cursor's mode and the viewport
    /// stay, because they are levels and the device is still doing them.
    ///
    /// The counterpart of [`absorb`](Self::absorb): a loop calls this once the
    /// tick that was owed the edge has run, which is what stops a frame that
    /// owes eight catch-up ticks from turning one keypress into eight actions.
    /// Between them the two make an edge reach exactly one tick, however many
    /// readings or ticks a frame happens to hold.
    pub fn settle(&mut self) {
        for slot in &mut self.digital {
            slot.pressed = false;
            slot.released = false;
        }
        self.delta.fill(Analog::ZERO);
        self.captured = None;
        self.focus.pressed = false;
        self.focus.released = false;
        self.text.clear();
    }

    /// Returns every device reading to released, zero and absent, keeping the
    /// pointer's mode, the capacity
    /// and the active set.
    ///
    /// This is for a runtime that holds one snapshot forever and refills it,
    /// which is what keeps a per-frame allocation out of the loop. It is not
    /// what switching sets does -- that is [`activate`](Self::activate), and it
    /// deliberately leaves the values alone.
    ///
    /// # The cursor survives, and the bug that says why
    ///
    /// [`cursor`](Self::cursor) is **not a device reading**. Everything else
    /// here is something a device did, which a fresh reading replaces; the
    /// pointer's mode is platform state published *into* the snapshot from the
    /// other direction, and nothing refills it on a frame where it did not
    /// change.
    ///
    /// Clearing it made [`cursor`](Self::cursor) permanently
    /// [`Free`](crate::Cursor::Free) for every game in the workspace. The order of a
    /// frame is: take the snapshot, ask the game what it wants the pointer to
    /// do, tell the platform, and **write back what actually took** -- into this
    /// snapshot, where the next frame's [`Devices::snapshot`] wiped it before
    /// anybody could read it. A game asking whether its lock had been granted
    /// got "no" for ever, whatever the platform had actually done.
    /// `corvid_window/tests/cursor.rs` opens a real window and is what would
    /// have caught it.
    ///
    /// [`absorb`](Self::absorb) already treated it as a level for the same
    /// reason. This is the two agreeing.
    ///
    /// [`viewport`](Self::viewport) is cleared, and the difference is worth
    /// being exact about: it is written afresh every frame from the target's
    /// own size, so there is nothing to preserve and "no display" is the honest
    /// answer for a snapshot nobody has told.
    ///
    /// [`Devices::snapshot`]: crate::platform::Devices::snapshot
    pub fn clear(&mut self) {
        self.digital.fill(Digital::RELEASED);
        self.analog.fill(Analog::ZERO);
        self.delta.fill(Analog::ZERO);
        self.poses.fill(None);
        self.pointer = None;
        self.viewport = None;
        self.captured = None;
        self.text.clear();
    }

    /// What was typed over this interval, in the order it was typed.
    ///
    /// An **interval** quantity, exactly like
    /// [`pressed`](Digital::pressed): [`absorb`](Self::absorb) appends and
    /// [`settle`](Self::settle) clears, so a character delivered between two
    /// ticks reaches exactly one tick rather than none or eight. That is what
    /// makes typing into a text field work at a tick rate slower than the
    /// display's, which is every tick rate.
    ///
    /// # This is not an action, and it is not bound
    ///
    /// Nothing here goes through the binding table. A key that is bound to an
    /// action raises that action; what a platform decided the same keystroke
    /// *spells* is a separate question, answered by a keyboard layout, a
    /// modifier and possibly an input method, and only the platform can answer
    /// it. A game reading this is reading text; a game reading
    /// [`digital`](Self::digital) is reading intent.
    ///
    /// So this is whatever the platform committed, which is a whole grapheme
    /// cluster at a time rather than a code point: the three Han characters an
    /// input method composed arrive from one commit and not three.
    ///
    /// # It does not reach the simulation
    ///
    /// Not by itself. Like everything else in a snapshot, what crosses into a
    /// tick is the `Action` a controller
    /// built -- so a game with a chat box puts the finished line in an action
    /// of its own rather than putting keystrokes on the wire.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Appends what a platform says was typed.
    ///
    /// Called by the window layer from a key event's committed text and from an
    /// input method's commit, and by a test standing in for either.
    pub fn type_text(&mut self, text: &str) {
        self.text.push_str(text);
    }
}
