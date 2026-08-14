//! The three ways a run is played, and the two settings that choose between
//! them.
//!
//! The seam against `launch.rs` is that the decision has already been made: by
//! the time one of these is called, the plan is built and the clock is chosen,
//! and what is left is which backend the loop is handed.

//! Playing: the three backends, and the plan each of them is handed.
//!
//! The seam against the builder is that this is where the settings stop being
//! settings. Nothing here writes a field; it reads what was set, decides which
//! backend the run wants, and hands the thread over to it.

#[cfg(feature = "window")]
use corvid_behavior::State;
use corvid_control::Controller as _;
use corvid_sound::Auralizer as _;
use corvid_time::{Elapsed, Step};

use crate::app::{App, Error, Outcome};
use crate::capture::Capture;
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
use crate::headless::Headless;
use crate::runtime::{Plan, Runtime};
use crate::settings::Settings;
#[cfg(feature = "render")]
use corvid_render::Render as _;

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    /// A run with a window: the binding table is resolved, the platform is
    /// handed an event loop, and the loop runs inside it.
    ///
    /// # Errors
    ///
    /// [`Error::Bound`](crate::Error::Bound) for a binding file that cannot be used,
    /// [`Error::NoWindow`](crate::Error::NoWindow) if the platform would not give us one, and whatever
    /// the run itself reported.
    #[cfg(feature = "window")]
    pub(super) fn run_windowed(
        mut self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        // the snapshot from the window's devices every frame, and what a
        // binding table is written against is which actions exist.
        let declaration = plan.input.sets();
        if declaration.is_empty() {
            // Not an error, because a game with genuinely no actions is a
            // legal thing to run and refusing it here would be this call
            // deciding that for it. Said out loud, because the alternative
            // is a window that opens, draws, and answers every input query
            // with `RELEASED` for the rest of the run with nothing anywhere
            // pointing at the missing line.
            tracing::warn!(
                name: "corvid_app.undeclared",
                "this run declares no action sets, so the window binds no key \
                 and no axis and every input query will read released; a \
                 windowed run wants `App::input` for its declaration even \
                 though the values are refilled from the devices",
            );
        }
        // The table this run is bound by, in the order the three sources
        // beat each other: the game's own `Controller::bindings` is the
        // author's answer, `App::bindings` is a harness overriding it for
        // one run, and the player's file beats both -- which is what makes
        // it a rebinding rather than a suggestion.
        let shipped = self
            .bindings
            .take()
            .unwrap_or_else(|| corvid_input::platform::Bindings::placeholder(declaration));
        let bindings = crate::controls::resolve(&plan.root, declaration, shipped)?;
        let config = corvid_window::Config::new(<G::State as State>::NAME, declaration)
            .icon(None)
            .bindings(bindings)
            .any_thread(self.any_thread);
        let pending = crate::windowed::Pending::<G> {
            controls: settings.controls.clone(),
            bot: settings.bot.clone(),
            graphics: settings.graphics.clone(),
            audio: settings.audio.clone(),
            settings,
            plan,
            capture,
            rate: self.rate,
            // A window runs in front of a player, so the default clock is
            // the wall. A `Fake` here would run the simulation at whatever
            // rate the display asked for frames.
            clock: self
                .clock
                .take()
                .unwrap_or_else(|| Box::new(corvid_time::Clock::wall())),
        };
        let host = corvid_window::run(config, crate::windowed::Windowed::<G>::new(pending))
            .map_err(|why| match why {
                corvid_window::Error::Opening(opening) => Error::NoWindow(opening),
                corvid_window::Error::Host(why) => why,
            })?;
        host.into_outcome()
    }

    /// A run with an adapter and no window, which is what writes pictures on a
    /// build machine.
    ///
    /// # Errors
    ///
    /// [`Error::Drew`](crate::Error::Drew) if the device would not open, and whatever the run
    /// itself reported.
    #[cfg(feature = "render")]
    pub(super) fn run_offscreen(
        self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        clock: Box<dyn Elapsed>,
        size: corvid_render::Extent,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        let renderer = corvid_render::Renderer::offscreen(size).map_err(Error::Drew)?;
        // The pipelines are built here because here is where the device is.
        let graphics = G::Render::new(
            corvid_render::Opened {
                device: renderer.device(),
                queue: renderer.queue(),
                format: renderer.format(),
            },
            settings.graphics.clone(),
        );
        // `false`: this is the offscreen path, which has an adapter and no
        // window. Nobody is in front of it, so nothing opens a sound card.
        Runtime::new(
            plan,
            crate::screen::Screen::<G>::new(renderer, capture, false),
            G::Controller::new(settings.controls.clone()),
            G::Bot::new(settings.bot.clone()),
            Some(graphics),
            G::Auralizer::new(settings.audio.clone()),
            settings,
        )?
        .drive(clock, Step::new(self.rate))
    }

    /// A run with no device at all.
    ///
    /// # Errors
    ///
    /// Whatever the run reported.
    pub(super) fn run_headless(
        self,
        plan: Plan<G::State>,
        capture: Option<Capture>,
        clock: Box<dyn Elapsed>,
        settings: Settings<G>,
    ) -> Result<Outcome<G>, Error> {
        // pipelines against and no renderer to hold. `None` is that, said
        // plainly, rather than a renderer built from a device that is not
        // there.
        drop(settings.graphics.clone());
        Runtime::new(
            plan,
            Headless::<G>::new(capture),
            G::Controller::new(settings.controls.clone()),
            G::Bot::new(settings.bot.clone()),
            Option::<G::Render>::None,
            G::Auralizer::new(settings.audio.clone()),
            settings,
        )?
        .drive(clock, Step::new(self.rate))
    }

    /// Opens a window and plays the game in it.
    ///
    /// The title is [`NAME`](corvid_behavior::State::NAME) and the icon is
    /// [`Render::icon`](corvid_render::Render::icon), because both are
    /// properties of the game rather than of this run: a game that spelled its
    /// name once here and once in the directory its saves land in would have
    /// two names.
    ///
    /// **The event loop takes the calling thread**, because on iOS, Android and
    /// the web it has to: the platform calls into the loop and a game that kept
    /// `main` would have nowhere to receive events. The loop is what calls this
    /// runtime, once per displayed frame.
    ///
    /// A windowed run refills its input snapshot from the window's devices, so
    /// the *values* [`input`](Self::input) carries are ignored -- those are what
    /// a headless run plays with and nothing fills them there. Its
    /// **declaration** is not ignored, and is required: the table an
    /// [`action_sets!`](corvid_input::action_sets) generated is what
    /// the window's binding table is written against and what sizes the
    /// snapshot it refills, and this is the only place a run is told about it.
    /// Call `.input(Input::new(action::SETS))` for a windowed run even though
    /// its values will be replaced -- the default is a snapshot over an empty
    /// table, which binds no key and no axis and answers
    /// [`RELEASED`](corvid_input::Digital::RELEASED) to every query
    /// for the whole run.
    ///
    /// The default clock becomes [`Clock::wall`](corvid_time::Clock::wall),
    /// because a window in front of a player runs in real time and the
    /// [`Clock::stepping`](corvid_time::Clock::stepping) a headless run defaults
    /// to would run the simulation as fast as the display asked for frames.
    ///
    /// # What this does not change
    ///
    /// The digest. The window fills an input snapshot and the game records
    /// `wgpu` calls, and neither of them is on the path from an action log to a
    /// state -- `tests/windowless.rs` runs the same opening with a device and
    /// without one and compares the traces.
    ///
    /// `const`, and it is worth saying why that is possible: asking for a
    /// window is one `bool`. Building the game's drawing half here instead
    /// would mean boxing it, since this would be the only context knowing
    /// `G: Render` -- and [`run`](Self::run) carries that bound itself, so there
    /// is nothing to capture.
    #[cfg(feature = "window")]
    #[must_use]
    pub const fn window(mut self) -> Self {
        self.windowed = true;
        self
    }

    /// Draws every frame into a texture `size` pixels across, with no window.
    ///
    /// The headless path with a real device on it: the same loop, the same
    /// `Render` implementation, the same trace, and an adapter actually
    /// rasterising it. It is what a build machine can run, it is the other
    /// half of the comparison that says a renderer cannot change a digest, and
    /// it is the only run that produces a picture -- a capture's PNG is read
    /// back off this texture.
    ///
    /// Ignored on a run that also asked for a [`window`](Self::window), which
    /// has a target of its own.
    #[cfg(feature = "render")]
    #[must_use]
    pub const fn offscreen(mut self, size: corvid_render::Extent) -> Self {
        self.offscreen = Some(size);
        self
    }
}
