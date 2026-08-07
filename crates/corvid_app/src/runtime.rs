//! The loop: what happens per tick, what happens per displayed frame, and
//! where the boundary between them is.

use crate::commands::Command;
use corvid_control::Controller;
use corvid_render::Render;
use corvid_replay::LevelRef;
use corvid_sound::Auralizer;
use std::{mem, sync::Arc};

use corvid_behavior::{ExitCode, Player, PlayerId, SaveSlot, Time};
use corvid_fixed::Factor16;
use corvid_hash::digest;
#[cfg(feature = "window")]
use corvid_input::Cursor;
use corvid_input::Input;
use corvid_replay::Session;
use corvid_signal::Emitter;
use corvid_sound::AudioFrame;
use corvid_time::Duration;
use corvid_time::{Clock, Step, Tick};

use crate::{
    Error, Outcome, Progress, Retention,
    app::Stop,
    backend::Backend,
    commands::{Answer, Sink},
    saves::{Saves, StateAt},
};
use corvid_behavior::State;

/// What a run reads its devices with when there are no devices.
///
/// [`App::inputs`](crate::App::inputs) is what fills one in, and its
/// documentation is where the reason lives. In short: a windowed run is refilled
/// once per displayed frame by the window, and without this a run with no window
/// plays the whole way through on one snapshot — so a menu press, a click and a
/// release cannot be written down in a test at all.
pub(crate) type Feed = Box<dyn FnMut(Tick) -> Input>;

/// Everything a run is, before it has a backend to display itself on.
///
/// One struct rather than seven arguments because there are three paths that
/// build a runtime — headless, offscreen and windowed — and the last of them
/// cannot build it until the platform hands over a window. Carrying the
/// ingredients as a value is what keeps the three from drifting: a setting added
/// here reaches all three or none.
pub(crate) struct Plan<S: State> {
    /// The session, already at its opening.
    pub(crate) session: Session<S>,
    /// Which seat this client's action is recorded against.
    pub(crate) seat: PlayerId,
    /// The transport the other machines are behind, for a run that has any.
    ///
    /// [`None`] is one seat and no network, which is what every example did
    /// before this existed and what a run that never calls
    /// [`App::transport`](crate::App::transport) still does.
    #[cfg(feature = "net")]
    pub(crate) transport: Option<Box<dyn corvid_net::Transport>>,
    /// How far ahead of the agreed frontier this machine will play, for a run
    /// with a transport.
    #[cfg(feature = "net")]
    pub(crate) budget: corvid_lockstep::Budget,
    /// What the devices say, for a run with no device layer under it.
    pub(crate) input: Input,
    /// When to stop, if the caller said.
    pub(crate) stop: Option<Stop<S>>,
    /// The tick to stop before, if the caller asked for a count.
    pub(crate) deadline: Option<Tick>,
    /// Where to publish progress, if anywhere.
    pub(crate) progress: Option<Emitter<Progress>>,
    /// How much of the session to keep as it is played.
    pub(crate) retention: Retention,
    /// Where a [`Save`](Command::Save) writes and a [`Read`](Command::Read)
    /// looks.
    pub(crate) saves: Saves,
    /// What the devices say, frame by frame, for a run whose caller is
    /// standing in for a player.
    pub(crate) feed: Option<Feed>,
    /// The tick the run opens at and the state there, for a run that was handed
    /// a session rather than starting one.
    ///
    /// [`None`] is a fresh session, which opens at
    /// [`Session::first`](corvid_replay::Session::first) on the opening's own
    /// origin state. A `--load` or a `--replay` fills this in, because the
    /// session it hands over has already been played and the state at its last
    /// tick is what the run carries on from.
    pub(crate) resumed: Option<StateAt<S>>,
}

/// How far back the run can still reach, and the state it would reopen at.
///
/// [`Retention`] is the setting; this is what the loop does with it. The kept
/// state is what makes a bounded run possible at all: a session cannot forget
/// its first rows without being handed the state at the tick it is left opening
/// on, and the only place that state exists is the loop that produced it.
enum Horizon<S: State> {
    /// Nothing is forgotten.
    Everything,
    /// A window, the tick the last state was set aside at, and that state.
    ///
    /// [`None`] until the run has been going for a whole window, which is why
    /// the first window's worth of ticks is never forgotten however small the
    /// window is: there is nothing yet to reopen at.
    Recent {
        /// How far back the run is sure to be able to reach.
        window: u64,
        /// The tick [`kept`](Self::Recent::kept) is the state at, or the tick
        /// the session opened on before there is one.
        marked: Tick,
        /// The state at [`marked`](Self::Recent::marked).
        kept: Option<Arc<S>>,
    },
}

/// Who is simulating: this machine alone, or this machine and the peers a
/// transport reaches.
///
/// The session is inside either arm rather than beside them, because a linked
/// run's session belongs to the [`Peer`](corvid_lockstep::Peer) — a rollback
/// rewrites the action log and the mark trace, and a second owner of those
/// would be a second answer to what the session is.
enum Play<S: State> {
    /// One seat, no network, and the loop writes its own action into the log
    /// and simulates it. Every run that names no transport.
    Local(Session<S>),
    /// A peer that predicts, rolls back and exchanges digests, and the
    /// transport its datagrams ride on.
    #[cfg(feature = "net")]
    Linked(Box<crate::net::Link<S>>),
}

impl<S: State> Play<S> {
    /// The session being played.
    fn session(&self) -> &Session<S> {
        match self {
            Self::Local(session) => session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.session(),
        }
    }

    /// The same, mutably, for the two things done to a session that are no part
    /// of simulating it: writing a save and forgetting the far past.
    fn session_mut(&mut self) -> &mut Session<S> {
        match self {
            Self::Local(session) => session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.session_mut(),
        }
    }

    /// The session, once the run is over.
    fn into_session(self) -> Session<S> {
        match self {
            Self::Local(session) => session,
            #[cfg(feature = "net")]
            Self::Linked(link) => link.into_session(),
        }
    }
}

/// Whether the loop carries on.
enum Flow {
    /// Keep going.
    Go,
    /// Stop, with this status.
    Stop(ExitCode),
}

/// Everything the loop reads and writes.
///
/// One struct rather than a pile of locals, so that the per-tick half and the
/// per-frame half are two functions rather than one long one. The two states
/// are handles because that is what a [`Frame`] holds: building one is four
/// atomic increments and no copy of a state, which is what makes it affordable
/// to build one per call rather than once per displayed frame.
pub(crate) struct Runtime<S: State, C, R, A, B> {
    /// The session being played, which is the run's whole output, and whoever
    /// is playing it.
    play: Play<S>,
    /// Which seat this client's action is recorded against.
    seat: PlayerId,
    /// The game's caches, carried from tick to tick.
    /// The state at [`at`](Self::at) minus one.
    previous: Arc<S>,
    /// The state at [`at`](Self::at).
    current: Arc<S>,
    /// Which tick [`current`](Self::current) is.
    at: Tick,
    /// The client-local half's state, moved only by `look`.
    /// Who is playing, and where they are looking.
    controller: C,
    /// What is drawn with, or [`None`] on a run that opened no device.
    ///
    /// An `Option` rather than a `R` because a renderer is built against a
    /// device and a headless run has none — so "there is no renderer" is the
    /// honest thing to hold rather than one built from nothing.
    graphics: Option<R>,
    /// The ear.
    ear: A,
    /// What the devices say, as of the last reading. This is the frame's input,
    /// and `look` is what reads it.
    input: Input,
    /// What no tick has spent yet: every edge and every displacement since the
    /// last tick, folded together. `intend` is what reads it.
    ///
    /// [`None`] for a run nobody refills, where the snapshot is whatever
    /// [`App::input`](crate::App::input) was given and is deliberately the same
    /// for every tick of the run. A windowed run fills it in
    /// [`refill`](Self::refill), because a window ends the edge interval once
    /// per displayed frame while the loop consumes it once per tick, and the
    /// two rates are not the same rate.
    unspent: Option<Input>,
    /// What the devices say, frame by frame, where a caller is standing in for
    /// a player. [`None`] for a windowed run, which is refilled by the window,
    /// and for a run whose snapshot never changes.
    feed: Option<Feed>,
    /// The one audio frame, kept for the life of the run and refilled per
    /// frame.
    audio: AudioFrame,
    /// Where a displayed frame goes.
    backend: B,
    /// What the ticks asked the platform for.
    sink: Sink<LevelRef<S>>,
    /// Where a save is written and read.
    saves: Saves,
    /// When to stop, if the caller said.
    stop: Option<Stop<S>>,
    /// The tick to stop *before*, if the caller asked for a fixed number of
    /// them. A count rather than a predicate, because the predicate is checked
    /// after a tick has run and `for_ticks(0)` has to mean no ticks at all.
    deadline: Option<Tick>,
    /// Where to publish progress, if the caller said.
    progress: Option<Emitter<Progress>>,
    /// How far back the session is kept.
    horizon: Horizon<S>,
}

impl<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>, B: Backend<S, R>>
    Runtime<S, C, R, A, B>
{
    /// Builds the loop's state from a session that is already at its opening.
    ///
    /// A [`Frame`] holds two states and there is only one before the first
    /// tick, so both ends of the run's opening pair are the same handle. That
    /// used to be two whole clones of the origin state; it is now two
    /// increments of one refcount, and the state itself is the one the opening
    /// already holds. The alternative — a `previous` that is an [`Option`] —
    /// would still put a branch in front of every extractor for the sake of the
    /// first frame.
    ///
    /// # Errors
    ///
    /// Only a linked run has one: [`Error::Halted`] if the state a `--load` or
    /// a `--replay` resumed at is one the peer will not adopt, which is a tick
    /// outside the session it was handed.
    pub(crate) fn new(
        mut plan: Plan<S>,
        backend: B,
        controller: C,
        graphics: Option<R>,
        ear: A,
    ) -> Result<Self, Error> {
        let resumed = plan.resumed.take();
        let (at, state) = resumed
            .clone()
            .unwrap_or_else(|| (plan.session.first(), plan.session.opening.origin()));
        let previous = Arc::clone(&state);
        let current = state;
        let horizon = match plan.retention {
            Retention::Everything => Horizon::Everything,
            Retention::Recent { ticks } => Horizon::Recent {
                window: ticks,
                marked: at,
                kept: None,
            },
        };

        // Who is playing. A transport is the whole of the difference, and the
        // resumed state is handed to the peer rather than only to the display
        // — a peer opening on the origin while the display showed a loaded
        // save would send digests for a session nobody else is in.
        #[cfg(feature = "net")]
        let playing = match plan.transport.take() {
            Some(transport) => {
                let mut link = Box::new(crate::net::Link::new(
                    plan.session,
                    plan.seat,
                    plan.budget,
                    transport,
                ));
                if let Some((at, state)) = resumed {
                    link.adopt(at, S::clone(&state))?;
                }
                Play::Linked(link)
            }
            None => Play::Local(plan.session),
        };
        #[cfg(not(feature = "net"))]
        let playing = {
            drop(resumed);
            Play::Local(plan.session)
        };

        Ok(Self {
            controller,
            graphics,
            ear,
            play: playing,
            seat: plan.seat,
            previous,
            current,
            at,
            input: plan.input,
            unspent: None,
            feed: plan.feed,
            audio: AudioFrame::new(),
            backend,
            sink: Sink::default(),
            saves: plan.saves,
            stop: plan.stop,
            deadline: plan.deadline,
            progress: plan.progress,
            horizon,
        })
    }

    /// Runs until a tick asks to quit or the caller's predicate says so.
    ///
    /// One iteration is one reading of the clock: the [`Step`] turns the
    /// elapsed time into a whole number of owed ticks, each of those runs, and
    /// then exactly one frame is displayed — including on the iteration the run
    /// stops on, so the last tick a capture holds a state for is also the last
    /// tick it holds a frame for.
    pub(crate) fn drive(
        mut self,
        mut clock: Box<dyn Clock>,
        mut step: Step,
    ) -> Result<Outcome<S>, Error> {
        self.publish(false);
        let exit = loop {
            let elapsed = clock.elapsed();
            if let Some(code) = self.pump(&mut step, elapsed)? {
                break code;
            }
        };
        self.publish(true);
        self.finish(exit)
    }

    /// One reading of the clock: every tick it owes, then exactly one frame.
    ///
    /// The whole body of [`drive`](Self::drive)'s loop, factored out because
    /// the windowed path cannot own a loop of its own — an event loop owns
    /// `main` — and calls this once per redraw instead. Both paths therefore do
    /// the same work in the same order per reading of the clock, which is what
    /// makes them the same run.
    ///
    /// Answers the status to stop with, or [`None`] to carry on.
    pub(crate) fn pump(
        &mut self,
        step: &mut Step,
        elapsed: Duration,
    ) -> Result<Option<ExitCode>, Error> {
        // Before anything reads the snapshot: a caller standing in for a player
        // hands over this frame's devices, and the backend says how big the
        // rectangle a pointer was reported against is.
        self.read_devices();

        // The one place a pause happens, and it happens by not advancing the
        // step rather than by throwing the owed ticks away. `elapsed` is the
        // interval since the last reading and the accumulator never sees it, so
        // a pause of ten minutes leaves the step exactly where it was and the
        // frame after the pause owes one tick — where an accumulator that had
        // gone on filling would owe nine thousand, of which the catch-up
        // ceiling would deliver eight at once and count the rest as dropped.
        let owed = if self.controller.simulating() {
            step.advance(elapsed)
        } else {
            0
        };
        // Where the display sits between the last tick and the next, read once
        // per reading of the clock because that is how many instants there are
        // in one: the ticks below all belong to the same one. A
        // `Fake::stepping` at exactly the period leaves this at zero forever,
        // which is why a headless capture is a sequence of endpoint states
        // rather than of interpolations.
        let alpha = step.alpha();

        let mut stopped = None;
        for _ in 0..owed {
            if let Flow::Stop(code) = self.advance()? {
                stopped = Some(code);
                break;
            }
            // An edge belongs to one tick. The reading it came from is the same
            // reading for all of these — there is no device to ask again in the
            // middle of a catch-up — so without this a frame that owes eight
            // ticks would turn one keypress into eight actions in eight
            // consecutive rows of the log.
            self.spend();
        }

        self.display(alpha, elapsed)?;
        Ok(stopped)
    }

    /// Where a displayed frame goes, for whoever has to tell it something the
    /// loop does not know about — a window's new size, for one.
    #[cfg(feature = "window")]
    pub(crate) const fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// What the game would like the pointer to be doing.
    ///
    /// Read once per frame by the windowed backend, which is the only one with
    /// a pointer to put anywhere: a headless run has no window, so nothing asks
    /// and the answer is never built.
    #[cfg(feature = "window")]
    pub(crate) fn cursor(&self) -> Cursor {
        self.controller.cursor()
    }

    /// Reads this frame's devices, and notes how big the target is.
    ///
    /// Two things a snapshot cannot know about itself, done in one place
    /// because both have to happen before `intend` and `look` see the frame:
    /// what a caller standing in for a player is holding down, and the
    /// rectangle its pointer was reported against.
    ///
    /// The viewport is written into both snapshots, because both are read: the
    /// frame's own by `look`, and the unspent one by `intend`. It is a level
    /// rather than an edge, so writing the current value over each of them is
    /// the whole of keeping them in step.
    fn read_devices(&mut self) {
        if let Some(feed) = self.feed.as_mut() {
            let fresh = feed(self.at);
            self.input.clone_from(&fresh);
            self.unspent
                .get_or_insert_with(|| Input::new(fresh.sets()))
                .absorb(&fresh);
        }
        let viewport = self.backend.viewport();
        self.input.set_viewport(viewport);
        if let Some(unspent) = self.unspent.as_mut() {
            unspent.set_viewport(viewport);
        }
    }

    /// Replaces what the devices say.
    ///
    /// The headless path never calls this — its snapshot is whatever
    /// [`App::input`](crate::App::input) was given, unchanged for the whole run
    /// unless [`App::inputs`](crate::App::inputs) named a source for it — and
    /// the windowed path calls it once per frame with what the window
    /// accumulated.
    ///
    /// Two snapshots come out of one reading, because the frame and the tick
    /// are not the same interval. `look` runs once per reading and wants this
    /// reading, edges and all; `intend` runs zero or more times per reading and
    /// wants every edge exactly once, which is what
    /// [`absorb`](Input::absorb) and [`settle`](Input::settle) between them
    /// arrange.
    #[cfg(feature = "window")]
    pub(crate) fn refill(&mut self, input: &Input) {
        self.input.clone_from(input);
        self.unspent
            .get_or_insert_with(|| Input::new(input.sets()))
            .absorb(input);
    }

    /// What `intend` reads: the unspent snapshot where a caller is refilling
    /// one, and the run's own otherwise.
    const fn acting(&self) -> &Input {
        match &self.unspent {
            Some(unspent) => unspent,
            None => &self.input,
        }
    }

    /// Spends the edges and displacements a tick has just consumed.
    ///
    /// Nothing at all for a run nobody refills, whose snapshot is documented as
    /// the same for every tick of the run.
    fn spend(&mut self) {
        if let Some(unspent) = self.unspent.as_mut() {
            unspent.settle();
        }
    }

    /// Publishes the last progress and hands the run back.
    #[cfg(feature = "window")]
    pub(crate) fn stop(self, exit: ExitCode) -> Result<Outcome<S>, Error> {
        self.publish(true);
        self.finish(exit)
    }

    /// One tick, and everything the tick owes.
    ///
    /// In order: build this client's action with `intend`, extend the log and
    /// record the action against this client's seat, simulate, digest the new
    /// state into the trace, let go of the state that falls out of the pair the
    /// display sits between, drain the commands into the sink, and ask whether
    /// to stop.
    ///
    /// The tick that asked for something is the tick whose `tick` returned it —
    /// `asked` below — and not the tick of the state it produced. That is the
    /// distinction behind "`Quit` stops the loop at the tick that asked": the
    /// state at `asked + 1` exists, because the tick that asked to quit is a
    /// tick that ran, and no tick after it does.
    /// # No alpha
    ///
    /// This used to be handed the display's interpolation weight, because
    /// `intend` was given a `Frame` that carried one. A `Controller::action` is
    /// given neither: interpolation is the renderer's and happens in a shader,
    /// so a tick's action cannot depend on where the display happens to sit.
    fn advance(&mut self) -> Result<Flow, Error> {
        let asked = self.at;
        // The count is checked on both sides of the tick, and each side is
        // there for a case the other misses. Before: `for_ticks(0)` is a run of
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
        let action = self
            .controller
            .action(&self.current, self.acting(), self.now());
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
            // would drop the commands after it in this same list — a `Quit`'s
            // status among them — and unwind past `finish`, leaving a capture
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
            .is_some_and(|stop| stop(&self.current, self.at))
        {
            return Ok(Flow::Stop(ExitCode::SUCCESS));
        }
        Ok(Flow::Go)
    }

    /// One tick with nobody else in the session: write this machine's action
    /// into the log, simulate it, and shift the displayed pair by one.
    ///
    /// This is what every run did before there was a transport, unchanged: the
    /// action is recorded against this client's seat, `tick` is called with the
    /// roster the session says was seated, and the digest of what came out goes
    /// into the trace.
    fn advance_alone(
        &mut self,
        asked: Tick,
        action: S::Action,
    ) -> Result<Vec<Command<LevelRef<S>>>, Error> {
        self.play
            .session_mut()
            .log
            .extend_to(asked)
            .map_err(Error::Log)?;
        let seat = self.seat;
        self.play
            .session_mut()
            .log
            .set(asked, seat, action)
            .map_err(Error::Log)?;

        let (next, commands) = self.simulate();
        self.play.session_mut().marks.push(digest(&next));

        // The pair the display sits between shifts by one, and what falls out
        // of the far end is dropped here — the last handle to it, unless an
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

    /// One tick with other machines in the session: submit, receive, advance,
    /// send, and follow the peer.
    ///
    /// **The peer is the authority on where the run is.** It may simulate one
    /// tick, none at all — when it is [`Budget::ahead`](corvid_lockstep::Budget)
    /// past the frontier every seat has confirmed, where predicting further
    /// would be predicting a decision — or it may land somewhere behind where
    /// it was, because a datagram corrected a prediction and the rollback went
    /// deeper than one tick. So the display's tick and its pair are read back
    /// off the peer rather than incremented here.
    #[cfg(feature = "net")]
    fn advance_linked(&mut self, action: S::Action) -> Result<Vec<Command<LevelRef<S>>>, Error> {
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
        // first time — a rollback's re-simulation reaches nothing, which is
        // `Peer::advance`'s rule rather than this loop's.
        let mut asked = crate::commands::Asked::default();
        link.play(action, &mut asked)?;

        let now = link.tick();
        let corrected = link.traffic().rolled.happened();
        if now != was || corrected {
            let state = Arc::new(S::clone(link.state()));
            if now == was.next() {
                // Ordinary forward play: the pair shifts by one, exactly as it
                // does with nobody else in the session.
                self.previous = mem::replace(&mut self.current, state);
            } else {
                // A rollback moved the state under the display. There is no
                // pair to interpolate across — the state a moment ago is one
                // this machine has decided never happened — so both ends
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

    /// Writes the session and the state at this tick into a slot.
    ///
    /// The whole of what a save is. A game implements nothing for it: its
    /// `State` is [`Data`](corvid_behavior::Data), so the runtime already has
    /// everything a save holds, and the bytes the request carries are the
    /// game's own record of the request rather than what goes in the file.
    ///
    /// A filesystem that refuses is [`Answer::Failed`] rather than the end of
    /// the run. The slot on disk is untouched — `Saves::write` renames a
    /// finished file over it or writes nothing at all — so the cost of carrying
    /// on is that the run has no save, which is what it would have had anyway,
    /// and the gain is that it still has its capture, its session and whatever
    /// the ticks after this one asked for. The failure is said out loud at
    /// `ERROR`, because a run that lost a player's save and mentioned it only in
    /// a value nobody printed would be worse than one that stopped.
    fn write_save(&self, at: Tick, slot: SaveSlot) -> Answer {
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
    /// operation — the same barrier a [`Load`](Command::Load) needs, and there
    /// is nothing here that raises one. What opens a session from a slot is
    /// `--load`, at start-up, where there is no session to interrupt.
    ///
    /// So this is the half that can be answered now: the runtime looked, and
    /// the slot either has a save in it or does not. Which is more than
    /// nothing — a game that offers a menu of slots needs exactly this to know
    /// which of them to draw.
    fn read_save(&self, at: Tick, slot: SaveSlot) -> Answer {
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
    /// called on is where the *next* horizon is put — the state at
    /// [`at`](Self::at) is [`current`](Self::current), which is the only place
    /// in the process it exists — and the one a window ago is where the session
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
    /// [`Session::last`](corvid_replay::Session::last) where it was —
    /// `tests/retention.rs` runs the same opening bounded and unbounded and
    /// compares the states, the marks and the actions over the overlap.
    fn forget_the_far_past(&mut self) {
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
    fn reached_the_count(&self) -> bool {
        self.deadline.is_some_and(|end| self.at >= end)
    }

    /// Calls the game's `tick` with the roster the session says was seated.
    ///
    /// The roster is rebuilt from the opening and the log every tick rather
    /// than kept, so that it is the same roster
    /// [`Session::seek`](corvid_replay::Session::seek) rebuilds from the same
    /// two things. A roster the runtime remembered would be a fourth input to
    /// the simulation that no capture records.
    fn simulate(&self) -> (S, Vec<Command<LevelRef<S>>>) {
        let idle = S::Action::default();
        let mut roster: Vec<Player<'_, S::Action>> = Vec::new();
        for (seat, profile) in self.play.session().opening.roster.iter().enumerate() {
            let Ok(seat) = u16::try_from(seat) else {
                break;
            };
            let id = PlayerId(seat);
            let Some(presence) = profile.presence_at(self.at) else {
                continue;
            };
            roster.push(Player {
                id,
                presence,
                action: self.play.session().log.get(self.at, id).unwrap_or(&idle),
            });
        }

        // A `Vec`-backed sink, which is what the trait's whole shape is for:
        // the runtime wants the requests in order so it can route and record
        // them, and a test wants exactly the same thing.
        let mut asked = crate::commands::Asked::default();
        let next = S::clone(&self.current).tick(
            &self.play.session().opening.content,
            &roster,
            &self.play.session().opening.rules,
            &mut asked,
        );
        drop(roster);
        (next, asked.0)
    }

    /// One displayed frame: advance the view, extract the sound, hand the
    /// frame to the backend.
    ///
    /// `dt` is the same interval the [`Step`] was advanced by, which is the
    /// only wall-clock quantity either half of a game ever sees and is the one
    /// [`look`](corvid_present::Present::look) is specified to take.
    fn display(&mut self, alpha: Factor16, dt: Duration) -> Result<(), Error> {
        // Three views of one instant, so one frame is built and cloned twice
        // rather than three being built from the same fields. A clone is four
        // atomic increments and no copy of a state, which is what a `Frame` of
        // handles buys and is why this is not worth writing any other way.
        //
        // It used to be one value passed three times because a `Frame` was
        // `Copy`, and the reason it could be built before the three calls at
        // all was that its borrows were disjoint fields of `self`. Neither
        // applies now: an owned frame borrows nothing, so `frame` is an
        // ordinary method call and the three calls below are free to take
        // `&mut self.view` in whatever order they like.
        let time = self.now();
        self.controller
            .update(&self.current, &self.input, None, time, dt);
        let camera = self.controller.look();

        // Extracted once per displayed frame, for the settled newest state —
        // never once per replayed tick. The renderer holds the pair and the
        // shader lerps between them with `alpha`.
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.extract(&self.current, &self.play.session().opening.content, time);
        }
        self.ear
            .extract(&self.current, &self.play.session().opening.content, time);

        self.audio.clear();
        self.ear.hear(&mut self.audio, &camera, time);
        // Nothing about the two states crosses this seam. The renderer already
        // holds whatever `extract` put in it; what goes over is the weight
        // between them.
        self.backend.present(
            self.at,
            self.graphics.as_mut(),
            &camera,
            None,
            time,
            alpha,
            &self.audio,
        )
    }

    /// Where the session is: the tick, and the wall clock since it opened.
    ///
    /// The elapsed half is zero for now. The loop's clock is a fixed step and
    /// what it accumulates is a debt of ticks rather than a duration since the
    /// opening, so there is nothing here to report yet that would not be a
    /// number this function invented.
    const fn now(&self) -> Time {
        Time {
            tick: self.at,
            elapsed: core::time::Duration::ZERO,
        }
    }

    /// Publishes where the run has got to, if anybody asked to be told.
    fn publish(&self, finished: bool) {
        if let Some(emitter) = &self.progress {
            emitter.set(Progress {
                tick: self.at,
                // The trace answers for every tick the loop advanced, which is
                // the normal path and a lookup rather than a hash. Computing
                // one is the fallback for a caller that replaced the trace,
                // where the state in hand is still the truth about `at`.
                mark: self
                    .play
                    .session()
                    .marks
                    .get(self.at)
                    .unwrap_or_else(|| digest(&*self.current)),
                frames: self.backend.frames(),
                finished,
            });
        }
    }

    /// Writes the capture's last two files and hands the run back.
    fn finish(self, exit: ExitCode) -> Result<Outcome<S>, Error> {
        #[cfg(feature = "net")]
        let traffic = match &self.play {
            Play::Local(_) => crate::Played::default(),
            Play::Linked(link) => link.played(),
        };
        let session = self.play.into_session();
        if let Some(capture) = self.backend.capture() {
            let bytes = session.save().map_err(|why| Error::Encoded {
                what: "a session",
                why,
            })?;
            capture.close(&bytes, &session.marks)?;
        }
        Ok(Outcome {
            session,
            state: self.current,
            exit,
            requests: self.sink.into_requests(),
            #[cfg(feature = "net")]
            traffic,
        })
    }
}
