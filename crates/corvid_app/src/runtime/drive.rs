//! Driving the loop from outside: what a windowed run calls per frame.
//!
//! The seam against `mod.rs` is who is in charge. Everything there is the
//! runtime setting itself up and reading its own devices; everything here is
//! called *by* a platform whose event loop owns the thread.

//! The loop: what happens per tick, what happens per displayed frame, and
//! where the boundary between them is.

use corvid_behavior::ExitCode;
use corvid_control::Controller as _;
#[cfg(feature = "window")]
use corvid_input::Cursor;
use corvid_input::Input;
use corvid_time::Duration;
use corvid_time::{Elapsed, Step};

use crate::{Error, Outcome, backend::Backend, game::Game};

use crate::runtime::{Flow, Runtime};

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Runs until a tick asks to quit or the caller's predicate says so.
    ///
    /// One iteration is one reading of the clock: the [`Step`] turns the
    /// elapsed time into a whole number of owed ticks, each of those runs, and
    /// then exactly one frame is displayed -- including on the iteration the run
    /// stops on, so the last tick a capture holds a state for is also the last
    /// tick it holds a frame for.
    pub(crate) fn drive(
        mut self,
        mut clock: Box<dyn Elapsed>,
        mut step: Step,
    ) -> Result<Outcome<G>, Error> {
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
    /// the windowed path cannot own a loop of its own -- an event loop owns
    /// `main` -- and calls this once per redraw instead. Both paths therefore do
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
        // frame after the pause owes one tick -- where an accumulator that had
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
        // `Clock::stepping` at exactly the period leaves this at zero forever,
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
            // reading for all of these -- there is no device to ask again in the
            // middle of a catch-up -- so without this a frame that owes eight
            // ticks would turn one keypress into eight actions in eight
            // consecutive rows of the log.
            self.spend();
        }

        self.display(alpha, elapsed)?;
        Ok(stopped)
    }

    /// Where a displayed frame goes, for whoever has to tell it something the
    /// loop does not know about -- a window's new size, for one.
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
    /// because both have to happen before `action` and `look` see the frame:
    /// what a caller standing in for a player is holding down, and the
    /// rectangle its pointer was reported against.
    ///
    /// The viewport is written into both snapshots, because both are read: the
    /// frame's own by `look`, and the unspent one by `action`. It is a level
    /// rather than an edge, so writing the current value over each of them is
    /// the whole of keeping them in step.
    pub(super) fn read_devices(&mut self) {
        let viewport = self.backend.viewport();
        self.input.set_viewport(viewport);
        if let Some(unspent) = self.unspent.as_mut() {
            unspent.set_viewport(viewport);
        }
    }

    /// Replaces what the devices say.
    ///
    /// The headless path never calls this -- its snapshot is whatever
    /// [`App::input`](crate::App::input) was given, unchanged for the whole run
    /// unless [`App::inputs`](crate::App::inputs) named a source for it -- and
    /// the windowed path calls it once per frame with what the window
    /// accumulated.
    ///
    /// Two snapshots come out of one reading, because the frame and the tick
    /// are not the same interval. `look` runs once per reading and wants this
    /// reading, edges and all; `action` runs zero or more times per reading and
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

    /// What `action` reads: the unspent snapshot where a caller is refilling
    /// one, and the run's own otherwise.
    pub(super) const fn acting(&self) -> &Input {
        match &self.unspent {
            Some(unspent) => unspent,
            None => &self.input,
        }
    }

    /// Spends the edges and displacements a tick has just consumed.
    ///
    /// Nothing at all for a run nobody refills, whose snapshot is documented as
    /// the same for every tick of the run.
    pub(super) fn spend(&mut self) {
        if let Some(unspent) = self.unspent.as_mut() {
            unspent.settle();
        }
    }

    /// Publishes the last progress and hands the run back.
    #[cfg(feature = "window")]
    pub(crate) fn stop(self, exit: ExitCode) -> Result<Outcome<G>, Error> {
        self.publish(true);
        self.finish(exit)
    }
}
