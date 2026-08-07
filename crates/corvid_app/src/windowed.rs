//! The windowed run: the same loop, driven by an event loop that owns `main`.

use crate::{
    Error, Outcome,
    capture::Capture,
    runtime::{Plan, Runtime},
    screen::Screen,
};
use corvid_behavior::ExitCode;
use corvid_behavior::State;
use corvid_control::Controller;
use corvid_input::{Cursor, Input};
use corvid_render::Render;
use corvid_render::{Extent, Renderer};
use corvid_signal::Seen;
use corvid_signal::Watch;
use corvid_sound::Auralizer;
use corvid_time::{Elapsed, Step, TickSpan};
use corvid_window::{Attached, Flow, Host, SurfaceState};

/// Everything a run needs that cannot be built until a window exists.
///
/// A renderer needs a surface and a surface needs a window, and a window does
/// not exist until the platform says so — which on Android is not at start-up
/// and can happen again after the app has been in the background. So the run's
/// ingredients wait here and become a [`Runtime`] in
/// [`attach`](Host::attach).
pub(crate) struct Pending<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> {
    /// What the three client-side types are built from, once the window and
    /// the device exist. Configs rather than instances, because only the event
    /// loop knows when that is.
    pub(crate) controls: C::Config,
    /// What the renderer is built from.
    pub(crate) graphics: R::Config,
    /// What the ear is built from.
    pub(crate) audio: A::Config,
    /// The run: the session, the seat, when to stop, and how much to keep.
    pub(crate) plan: Plan<S>,
    /// Where to write the run down, if anywhere.
    pub(crate) capture: Option<Capture>,
    /// How often a tick runs.
    pub(crate) rate: TickSpan,
    /// Where real time comes from.
    pub(crate) clock: Box<dyn Elapsed>,
}

/// The half of the program the event loop drives.
///
/// # What crosses the boundary
///
/// Down: a surface, a watch of the window's state, and one [`Input`] snapshot
/// per frame. Up: a [`Flow`] and an error. The window has no way to read a
/// state, a tick or a digest, and this type does not give it one — which is
/// why a windowed run and a headless run of the same opening land on the same
/// trace.
pub(crate) struct Windowed<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> {
    /// The ingredients, until the window exists.
    pending: Option<Pending<S, C, R, A>>,
    /// The loop, once it does.
    runtime: Option<Runtime<S, C, R, A, Screen<S>>>,
    /// The fixed step, carried across frames.
    step: Step,
    /// Where real time comes from.
    clock: Option<Box<dyn Elapsed>>,
    /// The window's published state, and how much of it has been noticed.
    surface: Option<(Watch<SurfaceState>, Seen)>,
    /// What the run ended with, once it has.
    finished: Option<Outcome<S>>,
    /// What went wrong on the way out, which is the one error a windowed run
    /// has nowhere else to put: [`Host::detach`] cannot fail.
    failed: Option<Error>,
    /// What the run stopped with, before the outcome was taken.
    exit: ExitCode,
}

impl<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> Windowed<S, C, R, A> {
    /// A host that has not opened anything yet.
    pub(crate) fn new(pending: Pending<S, C, R, A>) -> Self {
        let step = Step::new(pending.rate);
        Self {
            pending: Some(pending),
            runtime: None,
            step,
            clock: None,
            surface: None,
            finished: None,
            failed: None,
            exit: ExitCode::SUCCESS,
        }
    }

    /// What the run left behind.
    ///
    /// # Errors
    ///
    /// Whatever [`detach`](Host::detach) could not report, where the run played
    /// and could not be written down, and [`Error::NeverOpened`] where the
    /// window genuinely never opened. The two are worth telling apart: a run
    /// whose capture could not be closed has played every tick it was asked
    /// for, and answering it with "this run played no ticks" names the wrong
    /// thing and hides the disk that filled up.
    pub(crate) fn into_outcome(self) -> Result<Outcome<S>, Error> {
        if let Some(outcome) = self.finished {
            return Ok(outcome);
        }
        Err(self.failed.unwrap_or(Error::NeverOpened))
    }
}

impl<S: State, C: Controller<S>, R: Render<S>, A: Auralizer<S>> Host for Windowed<S, C, R, A> {
    type Error = Error;

    fn attach(&mut self, attached: &Attached) -> Result<Flow, Error> {
        let Some(pending) = self.pending.take() else {
            // The platform resumed us twice. The window is already open and the
            // run is already going; opening a second device would be a second
            // run of the same session.
            return Ok(Flow::Go);
        };

        let size = attached.surface.size();
        let renderer = Renderer::for_window(
            attached.surface.clone(),
            Extent::new(size.width, size.height),
            corvid_render::Pacing::Display,
        )
        .map_err(Error::Drew)?;

        let mut clock = pending.clock;
        // Read once and dropped, which is how a clock with no `reset` is reset.
        // It was started in `App::run`, before the event loop was built, before
        // the platform gave us a window and before the adapter and device above
        // were requested — routinely a second of start-up that is not simulated
        // time. Left in, the first frame owes the whole of it: at `CRADLE` a
        // two-second device request owes thirty ticks, `Step` delivers the
        // catch-up ceiling of them at once and counts the rest as dropped, so
        // every windowed run would open by simulating half a second the player
        // never saw and reporting a stall that never happened.
        let _ = clock.elapsed();
        self.clock = Some(clock);
        self.surface = Some((attached.state.clone(), attached.state.seen_now()));
        // A windowed run refills its snapshot from the window's devices every
        // frame, so whatever the plan carries is replaced before the first one
        // — and a scripted source is dropped, because the window is the device
        // layer and two of them would be two answers about one keyboard.
        let plan = Plan {
            input: Input::new(&[]),
            feed: None,
            ..pending.plan
        };
        // The pipelines are built here, which is the one place they can be:
        // the device exists and the window does, and neither did when the
        // `App` was described.
        let graphics = R::new(
            renderer.device(),
            renderer.queue(),
            renderer.format(),
            pending.graphics,
        );
        self.runtime = Some(Runtime::new(
            plan,
            // `true`: a window is the one backend with a player in front of it.
            Screen::new(renderer, pending.capture, true),
            C::new(pending.controls),
            Some(graphics),
            A::new(pending.audio),
        )?);
        Ok(Flow::Go)
    }

    fn frame(&mut self, input: &Input) -> Result<Flow, Error> {
        let (Some(runtime), Some(clock)) = (self.runtime.as_mut(), self.clock.as_mut()) else {
            return Ok(Flow::Go);
        };

        // The window publishes its size rather than calling in with it, so this
        // is where a resize is noticed: once per frame, at the latest value,
        // however many the platform reported since the last one.
        if let Some((watch, seen)) = &mut self.surface
            && let Some(state) = watch.changed_since(seen)
        {
            runtime
                .backend_mut()
                .resize(Extent::new(state.size.width, state.size.height));
        }

        runtime.refill(input);
        let elapsed = clock.elapsed();
        match runtime.pump(&mut self.step, elapsed)? {
            None => Ok(Flow::Go),
            Some(code) => {
                self.exit = code;
                Ok(Flow::Stop)
            }
        }
    }

    fn cursor(&self) -> Cursor {
        // Before the runtime exists there is no view to ask, and `Free` is what
        // a window does when nobody has said otherwise — so a game whose first
        // frame has not happened yet keeps a pointer the player can use.
        self.runtime.as_ref().map_or(Cursor::Free, Runtime::cursor)
    }

    fn detach(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        // A run that ends because the player closed the window still has to
        // write its capture and hand its session back, and this is the one call
        // that happens whatever stopped the loop.
        match runtime.stop(self.exit) {
            Ok(outcome) => self.finished = Some(outcome),
            Err(why) => {
                tracing::error!(
                    name: "corvid_app.unfinished",
                    why = %why,
                    "the run could not be written down on the way out",
                );
                // Kept as well as logged. `stop` takes the runtime by value, so
                // the session, the final state and every request have gone with
                // the attempt; a caller that saw only `NeverOpened` would be
                // told a run that opened a window and played every tick never
                // started, and could not tell it from one that really did not.
                self.failed = Some(why);
            }
        }
    }
}
