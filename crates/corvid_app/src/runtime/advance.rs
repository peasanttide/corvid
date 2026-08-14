//! One tick: simulated alone, or agreed with the peers first.
//!
//! The seam against `display.rs` is the clock a step belongs to. Everything
//! here happens once per *tick*, which is what a session agrees about;
//! everything there happens once per *displayed frame*, which no two machines
//! agree about.

use std::{mem, sync::Arc};

use corvid_behavior::{ExitCode, State};
use corvid_control::{Acting, Controller};
use corvid_hash::digest;
use corvid_time::Tick;

use crate::Error;
use crate::backend::Backend;
use crate::commands::Command;
use crate::game::Game;
use crate::runtime::{Flow, Play, Runtime};

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// One tick, and everything the tick owes.
    ///
    /// In order: ask [`Controller::action`] for this client's action, extend
    /// the log and record that action against this client's seat, simulate,
    /// digest the new state into the trace, let go of the state that falls out
    /// of the pair the display sits between, drain the commands into the sink,
    /// and ask whether to stop.
    ///
    /// The first two of those are what a client that plays nobody skips. There
    /// is no action to ask for and none to record, so [`Controller::action`] is
    /// not called and the row the log grows is left holding
    /// [`Action::default`](Default::default) -- which is what the seat holds
    /// anyway when a peer or a bot has not filled it. Everything after the
    /// comma happens exactly as it does for a client that plays.
    ///
    /// The tick that asked for something is the tick whose `tick` returned it --
    /// `asked` below -- and not the tick of the state it produced. That is the
    /// distinction behind "`Quit` stops the loop at the tick that asked": the
    /// state at `asked + 1` exists, because the tick that asked to quit is a
    /// tick that ran, and no tick after it does.
    /// # No alpha
    ///
    /// A `Controller::action` is handed no interpolation weight and no frame:
    /// interpolation is the renderer's and happens in a shader, so a tick's
    /// action cannot depend on where the display happens to sit.
    pub(super) fn advance(&mut self) -> Result<Flow, Error> {
        let asked = self.at;
        // The count is checked on both sides of the tick, and each side is
        // there for a case the other misses. Before: no ticks at all is a run of
        // no ticks, and a check that only ran afterwards would have simulated
        // the one it was asked not to. After: the run has to stop on the
        // iteration whose tick reached the count, because stopping on the next
        // one would read the clock again and display a frame that no tick of
        // this run produced.
        if self.reached_the_count() {
            return Ok(Flow::Stop(ExitCode::SUCCESS));
        }

        // The one call that is the same on both paths, and the reason a game
        // implements nothing to play over a network: what goes on the wire is
        // whatever this returns.
        //
        // Not made at all by a client that plays nobody. A spectator with no
        // action to submit has no question to ask its controller, and asking
        // anyway would run a game's decision code once per tick to throw the
        // answer away.
        let action = self.seating.playing().map(|seat| {
            self.controller.action(Acting {
                state: &self.current,
                input: self.acting(),
                time: self.now(),
                seat,
            })
        });
        let commands = match &mut self.play {
            Play::Local(_) => self.advance_alone(asked, action)?,
            #[cfg(feature = "net")]
            Play::Linked(_) => self.advance_linked(action)?,
        };
        self.forget_the_far_past();
        self.publish(false);

        for command in commands {
            // The two the loop is the only thing that can act on, because both
            // are about the session and the state and the sink holds neither.
            //
            // Neither can abort the tick. A filesystem that refuses is a fact
            // about the machine rather than about the simulation, and a `?` here
            // would drop the commands after it in this same list -- a `Quit`'s
            // status among them -- and unwind past `finish`, leaving a capture
            // with frames in it and no session or trace to replay them against.
            let answered = match &command {
                Command::Save(slot) => Some(self.write_save(asked, *slot)),
                Command::Read(slot) => Some(self.read_save(asked, *slot)),
                _ => None,
            };
            self.sink.absorb(asked, command, answered);
        }
        if let Some(code) = self.sink.quit() {
            return Ok(Flow::Stop(code));
        }
        if self.reached_the_count() {
            return Ok(Flow::Stop(ExitCode::SUCCESS));
        }
        // The state at `self.at`, and `self.at` itself: a predicate that wants
        // to stop after a number of ticks reads the second and a game keeps no
        // counter of its own for it.
        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.reached(&self.current, self.at))
        {
            return Ok(Flow::Stop(ExitCode::SUCCESS));
        }
        Ok(Flow::Go)
    }

    /// One tick with nobody else in the session: write this machine's action
    /// into the log, simulate it, and shift the displayed pair by one.
    ///
    /// The action is recorded against this client's seat, `tick` is called with
    /// the roster the session says was seated, and the digest of what came out
    /// goes into the trace.
    ///
    /// The row is grown either way and the write is what a spectator skips: a
    /// row nobody wrote reads [`Action::default`](Default::default), which is
    /// what a seat nobody is submitting for holds, and it is the same row a
    /// seat driven by a bot or by a peer would have had filled in.
    ///
    /// The bots' seats are written into the same row, after this client's, and
    /// [`play_bots`](Self::play_bots) says why the order does not matter.
    pub(super) fn advance_alone(
        &mut self,
        asked: Tick,
        action: Option<<G::State as State>::Action>,
    ) -> Result<Vec<Command>, Error> {
        self.play
            .session_mut()
            .log
            .extend_to(asked)
            .map_err(Error::Log)?;
        if let Some((seat, action)) = self.seating.playing().zip(action) {
            self.play
                .session_mut()
                .log
                .set(asked, seat, action)
                .map_err(Error::Log)?;
        }
        self.play_bots(asked)?;

        let (next, commands) = self.simulate();
        self.play.session_mut().marks.push(digest(&next));

        // The pair the display sits between shifts by one, and what falls out
        // of the far end is dropped here -- the last handle to it, unless an
        // extractor put a `Frame` somewhere that outlives this tick, which is
        // exactly the thing an owned `Frame` is allowed to do and the reason
        // nothing is handed back to the game by value any more.
        drop(mem::replace(
            &mut self.previous,
            mem::replace(&mut self.current, Arc::new(next)),
        ));
        self.at = asked.next();
        Ok(commands)
    }

    /// Asks the bot for an action for each seat it plays, and records them.
    ///
    /// Nothing at all for a run that asked for no bots, which is the whole of
    /// what such a run pays: the list is empty, the loop does not run, and the
    /// session is the one it would have been.
    ///
    /// The bot is asked with the state at [`at`](Self::at) and the same input
    /// snapshot this client's controller was handed, which is what makes each
    /// answer a function of the tick rather than of the order the seats are
    /// written in. It is one bot rather than one per seat, and
    /// [`Acting::seat`] is what tells it which it is answering for.
    ///
    /// The seats are indexed rather than iterated because deciding an action
    /// reads the whole runtime and recording it needs the session mutably: a
    /// `for` over the field would hold a shared borrow of `self` across the
    /// write.
    pub(super) fn play_bots(&mut self, asked: Tick) -> Result<(), Error> {
        let mut index = 0;
        while let Some(seat) = self.bots.get(index).copied() {
            index += 1;
            let action = self.bot.action(Acting {
                state: &self.current,
                input: self.acting(),
                time: self.now(),
                seat,
            });
            self.play
                .session_mut()
                .log
                .set(asked, seat, action)
                .map_err(Error::Log)?;
        }
        Ok(())
    }

    /// One tick with other machines in the session: submit, receive, advance,
    /// send, and follow the peer.
    ///
    /// **The peer is the authority on where the run is.** It may simulate one
    /// tick, none at all -- when it is [`Budget::ahead`](corvid_lockstep::Budget)
    /// past the frontier every seat has confirmed, where predicting further
    /// would be predicting a decision -- or it may land somewhere behind where
    /// it was, because a datagram corrected a prediction and the rollback went
    /// deeper than one tick. So the display's tick and its pair are read back
    /// off the peer rather than incremented here.
    ///
    /// [`None`] is a client that plays nobody: the peer submits nothing and
    /// only receives, predicts and simulates. That is expressible because
    /// submitting is a call of its own -- [`Peer::submit`](corvid_lockstep::Peer::submit)
    /// -- rather than an argument to
    /// [`advance`](corvid_lockstep::Peer::advance).
    ///
    /// **It is not a spectator mode for a session with an empty seat in it, and
    /// it is not safe at [`Budget::delay`](corvid_lockstep::Budget) zero.** A
    /// column nobody writes pins
    /// [`Frontier::agreed`](corvid_lockstep::Frontier::agreed) at
    /// [`Tick::ZERO`] -- a seat that has confirmed nothing counts as zero rather
    /// than as the session's first tick -- and stalls every peer in the session
    /// after [`Budget::ahead`](corvid_lockstep::Budget) ticks, this one
    /// included, which for a resumed session is the tick it opened on; and
    /// at `delay == 0` the idle row a spectator's datagram carries for the
    /// watched seat collides with the real action the machine in that seat
    /// wrote for the same tick, which is
    /// [`Halt::Contradiction`](corvid_lockstep::Halt).
    /// [`Link::play`](crate::net::Link::play) has the mechanisms and what would
    /// answer each.
    #[cfg(feature = "net")]
    pub(super) fn advance_linked(
        &mut self,
        action: Option<<G::State as State>::Action>,
    ) -> Result<Vec<Command>, Error> {
        let was = self.at;
        let Play::Linked(link) = &mut self.play else {
            // Reached only if this were called on a local run, which the one
            // call site's `match` rules out. Answering "nothing happened" is
            // the honest form of that: the workspace denies `unreachable!`,
            // and a run that quietly did no tick is better than one that
            // stopped on a branch nobody can take.
            return Ok(Vec::new());
        };

        // The peer's own sink, filled by whichever ticks it simulated for the
        // first time -- a rollback's re-simulation reaches nothing, which is
        // `Peer::advance`'s rule rather than this loop's.
        let mut asked = crate::commands::Asked::default();
        link.play(action, &mut asked)?;

        let now = link.tick();
        let corrected = link.traffic().rolled.happened();
        if now != was || corrected {
            let state = Arc::new(<G::State>::clone(link.state()));
            if now == was.next() {
                // Ordinary forward play: the pair shifts by one, exactly as it
                // does with nobody else in the session.
                self.previous = mem::replace(&mut self.current, state);
            } else {
                // A rollback moved the state under the display. There is no
                // pair to interpolate across -- the state a moment ago is one
                // this machine has decided never happened -- so both ends
                // become the corrected state and the next tick opens a fresh
                // pair. A client that interpolated from the discarded state
                // would draw the correction as motion.
                self.previous = Arc::clone(&state);
                self.current = state;
            }
            self.at = now;
        }
        Ok(asked.0)
    }
}
