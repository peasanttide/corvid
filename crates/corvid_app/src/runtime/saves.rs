//! The two requests the loop is the only thing that can act on.
//!
//! The seam is what a save needs: the session and the state, neither of which
//! the request sink holds. So these two are answered here and the answer is
//! handed back to the sink, which records them beside every other request.

use std::{mem, sync::Arc};

use corvid_behavior::{PlayerId, PlayerState, SaveSlot, State};
use corvid_time::Tick;

use crate::backend::Backend;
use crate::commands::Answer;
use crate::game::Game;
use crate::runtime::{Horizon, Runtime, Ticked};

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Writes the session and the state at this tick into a slot.
    ///
    /// The whole of what a save is. A game implements nothing for it: its
    /// `State` is [`Data`](corvid_behavior::Data), so the runtime already has
    /// everything a save holds, and the bytes the request carries are the
    /// game's own record of the request rather than what goes in the file.
    ///
    /// A filesystem that refuses is [`Answer::Failed`] rather than the end of
    /// the run. The slot on disk is untouched -- `Saves::write` renames a
    /// finished file over it or writes nothing at all -- so the cost of carrying
    /// on is that the run has no save, which is what it would have had anyway,
    /// and the gain is that it still has its capture, its session and whatever
    /// the ticks after this one asked for. The failure is said out loud at
    /// `ERROR`, because a run that lost a player's save and mentioned it only in
    /// a value nobody printed would be worse than one that stopped.
    pub(super) fn write_save(&self, at: Tick, slot: SaveSlot) -> Answer {
        match self.saves.write(slot, self.play.session(), &self.current) {
            Ok(()) => Answer::Done,
            Err(why) => {
                tracing::error!(
                    name: "corvid_app.unsaved",
                    tick = %at,
                    slot = slot.0,
                    why = %why,
                    "this save could not be written, so the slot still holds whatever it \
                     held before; the run carries on and the request is answered as failed",
                );
                Answer::Failed
            }
        }
    }

    /// Answers whether there is a save in a slot.
    ///
    /// **A read does not reopen the run.** What a save holds is a whole
    /// session, and putting one in front of a simulation that is already
    /// playing another is a barrier across every peer rather than a file
    /// operation -- the same barrier a [`Load`](Command::Load) needs, and there
    /// is nothing here that raises one. What opens a session from a slot is
    /// `--load`, at start-up, where there is no session to interrupt.
    ///
    /// So this is the half that can be answered now: the runtime looked, and
    /// the slot either has a save in it or does not. Which is more than
    /// nothing -- a game that offers a menu of slots needs exactly this to know
    /// which of them to draw.
    pub(super) fn read_save(&self, at: Tick, slot: SaveSlot) -> Answer {
        match self.saves.holds(slot) {
            Ok(true) => Answer::Done,
            Ok(false) => Answer::Empty,
            // A directory that will not say what is in it, which is not the
            // same finding as an empty slot and is not the end of the run
            // either. `write_save` says why.
            Err(why) => {
                tracing::error!(
                    name: "corvid_app.unread",
                    tick = %at,
                    slot = slot.0,
                    why = %why,
                    "this slot could not be looked at; the run carries on and the request \
                     is answered as failed",
                );
                Answer::Failed
            }
        }
    }

    /// Lets the session forget everything before the last state set aside, once
    /// a whole window has gone by since that state was set aside.
    ///
    /// Two ticks matter here and they are a window apart. The one this is
    /// called on is where the *next* horizon is put -- the state at
    /// [`at`](Self::at) is [`current`](Self::current), which is the only place
    /// in the process it exists -- and the one a window ago is where the session
    /// is reopened. Keeping a state aside is what makes the whole thing
    /// possible: a session cannot forget its first rows without being handed the
    /// state at the tick it is left opening on, and re-deriving that state would
    /// mean replaying the very rows being thrown away.
    ///
    /// So a run holds between one window and two of them, and what it pays per
    /// window is an increment of a refcount: the state set aside is the handle
    /// the loop is already holding as [`current`](Self::current), so a bounded
    /// run costs no copy of a state at all. The one that falls out of reach is
    /// dropped, and the memory comes back with the last handle to it rather
    /// than at any point this function can name.
    ///
    /// Nothing here can change what the run computes. The rows this drops are
    /// behind the frontier the loop writes at, `tick` is never handed anything
    /// but the current row, and `Session::forget_before` leaves
    /// [`Session::last`](corvid_replay::Session::last) where it was --
    /// `tests/retention.rs` runs the same opening bounded and unbounded and
    /// compares the states, the marks and the actions over the overlap.
    pub(super) fn forget_the_far_past(&mut self) {
        let Horizon::Recent {
            window,
            marked,
            kept,
        } = &mut self.horizon
        else {
            return;
        };
        if self.at.since(*marked) < *window {
            return;
        }

        let horizon = mem::replace(marked, self.at);
        let Some(origin) = kept.replace(Arc::clone(&self.current)) else {
            // The first window of a run has no earlier state to reopen at, so
            // there is nothing to forget yet.
            return;
        };

        match self.play.session_mut().forget_before(horizon, origin) {
            // The origin the session was holding until a moment ago, handed
            // back rather than dropped inside `forget_before` so that a caller
            // which had a use for it has the chance. This one has none.
            Ok(retired) => drop(retired),
            // Both refusals are a tick outside the session, and this one is a
            // tick the run itself reached and has not passed. It is reported
            // rather than dropped for the reason the command sink is: a runtime
            // with a gap in it should say so where somebody can read it.
            Err(why) => tracing::warn!(
                name: "corvid_app.unforgotten",
                tick = %horizon,
                why = %why,
                "the session would not forget its far past, so this run keeps growing",
            ),
        }
    }

    /// Whether the run has simulated as many ticks as it was asked for.
    ///
    /// Always false for a run whose caller named no count, which is every run
    /// stopped by a predicate or by a [`Quit`](corvid_behavior::Command::Quit).
    pub(super) fn reached_the_count(&self) -> bool {
        self.deadline.is_some_and(|end| self.at >= end)
    }

    /// Calls the game's `tick` with the roster the session says was seated.
    ///
    /// The roster is rebuilt from the opening and the log every tick rather
    /// than kept, so that it is the same roster
    /// [`Session::seek`](corvid_replay::Session::seek) rebuilds from the same
    /// two things. A roster the runtime remembered would be a fourth input to
    /// the simulation that no capture records.
    pub(super) fn simulate(&self) -> Ticked<G> {
        let idle = <<G::State as State>::Action>::default();
        let mut roster: Vec<PlayerState<<G::State as State>::Action>> = Vec::new();
        for (seat, profile) in self.play.session().opening.roster.iter().enumerate() {
            let Ok(seat) = u16::try_from(seat) else {
                break;
            };
            let id = PlayerId(seat);
            let Some(presence) = profile.presence_at(self.at) else {
                continue;
            };
            roster.push(PlayerState {
                id,
                presence,
                action: self
                    .play
                    .session()
                    .log
                    .get(self.at, id)
                    .cloned()
                    .unwrap_or_else(|| idle.clone()),
            });
        }

        // A `Vec`-backed sink, which is what the trait's whole shape is for:
        // the runtime wants the requests in order so it can route and record
        // them, and a test wants exactly the same thing.
        let mut asked = crate::commands::Asked::default();
        let next = <G::State>::clone(&self.current).tick(
            &self.play.session().opening.content,
            &roster,
            &self.play.session().opening.rules,
            &mut asked,
        );
        drop(roster);
        (next, asked.0)
    }
}
