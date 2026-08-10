//! What a player is doing, where they are looking, and what their pad feels.

use core::time::Duration;

use corvid_behavior::{Data, Loading, PlayerId, RumbleId, State};
use corvid_camera::Camera;
use corvid_input::{Cursor, Input, SetDescriptor, platform::Bindings};
use corvid_time::Time;

/// What a controller is handed when it is asked for an action.
///
/// One struct rather than four arguments, so that a new thing to hand over is a
/// field here and not a signature change in every implementation.
///
/// [`Copy`], because a bot answering for several seats is handed one of these
/// per seat in the same tick.
///
/// Written by hand rather than derived: a derive puts `S: Copy` on the impl,
/// because it goes by which type parameters appear rather than by what the
/// fields actually hold. Every field here is a shared reference, a `Time` or a
/// `PlayerId`, copy regardless of whether `S` is.
#[derive(Debug)]
pub struct Acting<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// What the devices say, with every edge since the last tick folded in.
    pub input: &'a Input,
    /// Where the session is.
    pub time: Time,
    /// Which seat this answer is for.
    ///
    /// A controller playing one seat reads it or ignores it; a bot answering
    /// for several is called once per seat and this is how it tells them apart.
    /// It is here rather than on [`Time`] because a seat is not something a
    /// tick may read.
    pub seat: PlayerId,
}

#[allow(
    clippy::expl_impl_clone_on_copy,
    reason = "a derive would add S: Clone, which is not true of every game's state and not needed by any field here"
)]
impl<S: State> Clone for Acting<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: State> Copy for Acting<'_, S> {}

/// What a controller is handed once per displayed frame.
#[derive(Debug)]
pub struct Updating<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// What the devices say.
    pub input: &'a Input,
    /// How far along this machine's bytes are, while a level is being read.
    pub loading: Option<Loading<'a>>,
    /// Where the session is.
    pub time: Time,
    /// Real time since the last displayed frame.
    pub dt: Duration,
    /// Which seat this controller is looking through.
    ///
    /// A controller playing one seat reads it or ignores it; a bot updating
    /// several is called once per seat and this is how it tells them apart.
    /// It is here rather than on [`Time`] because a seat is not something a
    /// frame may read.
    pub seat: PlayerId,
}

/// The half of a game's client-local code that reads a player.
///
/// One of the five types a `Game` is made of, and the one that is a *player*.
/// A game may have several: a keyboard and a pad is one, a scripted opponent is
/// another, a replay is a third, and each is a different type implementing this
/// trait rather than a flag inside one. Two of the five are this trait, because
/// a bot is a controller with nobody behind it.
///
/// Nothing here is deterministic and nothing here has to be. What crosses back
/// into the simulation is one value — the
/// [`Action`](corvid_behavior::State::Action) that [`action`](Self::action)
/// returns — and that is why a camera may be read here, and a wall clock may be
/// read here, and neither costs anything on the wire.
///
/// # `update` writes, everything else reads
///
/// | Function | Runs | Writes |
/// |---|---|---|
/// | [`update`](Self::update) | once per **displayed frame** | `&mut self` |
/// | [`look`](Self::look) | once per displayed frame, after `update` | nothing |
/// | [`action`](Self::action) | once per **tick** | nothing |
/// | [`rumble`](Self::rumble) | once per tick | nothing |
///
/// A fifteen-hertz simulation on a hundred-and-forty-four-hertz display calls
/// the bottom two fifteen times a second and the top two nine or ten times as
/// often, at a rate nobody chose and nothing records.
///
/// **That split is the point.** One method mutates and three read, so a `look`
/// that moved the camera it was reporting stops being expressible. Handing all
/// four a shared `&View` instead would only owe that discipline rather than
/// enforce it: a type bounded by `Default` may hold a
/// [`Cell`](core::cell::Cell), so every one of the reading functions could
/// write through it, and "the view moves in one place" would be a paragraph
/// instead of a signature.
///
/// # What goes on the wire is the action, not what it read
///
/// [`action`](Self::action) may read the camera, the cursor and the clock,
/// because none of them leaves this machine. A ray cast from this machine's
/// pointer, resolved against this machine's camera, arrives at every other peer
/// as `Action::Aim { at }` — a value in the game's own vocabulary, which every
/// peer folds into the state the same way.
///
/// So the rule is about what the *action* denotes rather than about what
/// `action` may look at. An action that names a target is fine. An action that
/// names a screen pixel, a viewport-relative offset or a number of display
/// frames is not, and nothing here can tell those apart: they are the same type
/// as far as the compiler is concerned, and the second kind desyncs nothing and
/// simply means something different on a machine with a different window.
pub trait Controller<S: State> {
    /// The player's own settings: sensitivity, invert-Y, dead zones.
    ///
    /// [`Data`], so the settings file is the same shape as the type. This is
    /// the half of a game's configuration that is **not**
    /// [`Rules`](corvid_behavior::State::Rules): it changes what one player
    /// feels and never what the simulation computes, so it is hashed by nothing
    /// and sent to nobody.
    type Config: Data;

    /// Whether this controller wants the platform's input devices.
    ///
    /// `false` means the runtime opens no window and reads no keyboard, and is
    /// what makes a bot, a replay and a dedicated server cost nothing. The
    /// other methods are still called — a bot has to answer
    /// [`action`](Self::action) — so this is about the *platform*, not about
    /// whether the controller runs.
    const REAL: bool = true;

    /// Every action set this game declares, which is the `SETS` that
    /// [`action_sets!`](corvid_input::action_sets) generated.
    ///
    /// **Required, with no default, and that is the whole point of it.** The
    /// declaration is what a snapshot is sized from and what a binding table is
    /// written against. Defaulting it would let a game played through the
    /// universal `main` run with an empty declaration, which binds no key and
    /// no axis and answers [`Digital::RELEASED`](corvid_input::Digital) to
    /// every query for the length of the run — a silent failure with nothing
    /// anywhere pointing at the missing line.
    ///
    /// A controller with genuinely no actions writes `&[]` and means it, which
    /// is what a scripted one does.
    const SETS: &'static [SetDescriptor];

    /// Build one from the player's settings.
    fn new(config: Self::Config) -> Self;

    /// The settings changed while the game was running.
    ///
    /// A menu moved a slider, or a config file was reloaded. The default takes
    /// the new settings by rebuilding, which is right for a controller holding
    /// nothing it would mind losing and wrong for one holding a camera — so a
    /// controller with state overrides this and keeps it.
    fn configure(&mut self, config: Self::Config)
    where
        Self: Sized,
    {
        *self = Self::new(config);
    }

    /// Which control drives which action, before a player has rebound anything.
    ///
    /// The table this game *ships*. It is client-local and cannot move a
    /// digest: a binding decides which control raises which action, and a peer
    /// who bound a different key submits the same action when they press it.
    ///
    /// The default binds by identifier *number* and therefore has no idea what
    /// any action means. It is honestly good for one thing: a game with a
    /// window opens and something happens when a key is pressed.
    ///
    /// What overrides this is the player: a binding file on disk is read after
    /// this is asked for, and replaces it entirely.
    #[must_use]
    fn bindings() -> Bindings {
        Bindings::placeholder(Self::SETS)
    }

    /// What the player has set, if this controller has changed it.
    ///
    /// Read after every [`update`](Self::update). [`Some`] is "write this
    /// down": the runtime persists the whole settings document to
    /// `$XDG_CONFIG_HOME/<NAME>/setting.json` when the answer differs from what
    /// it last wrote, so a settings menu that rebinds a key in `update` is a
    /// settings menu whose rebinding survives the run.
    ///
    /// [`None`], which is the default, is "nothing to write" — and it is the
    /// right answer for the overwhelming majority of controllers, which read a
    /// config at construction and never change it. A controller answering
    /// [`Some`] unconditionally would have the runtime comparing a whole config
    /// every frame for a game that never edits one.
    ///
    /// The runtime writes rather than the controller, for the reason nothing in
    /// this ring opens a file: a controller that wrote its own settings would be
    /// one that cannot run headless, cannot run twice in one process, and does
    /// its filesystem work from inside a display frame.
    fn config(&self) -> Option<Self::Config> {
        None
    }

    /// Advance client-local state, once per displayed frame.
    ///
    /// Where the camera moves, the cursor raycasts, and cosmetic state that is
    /// neither the renderer's nor the ear's is kept. **The only `&mut self`
    /// hook there is**, so everything this controller accumulates accumulates
    /// here.
    ///
    /// `dt` is real time since the last displayed frame — a wall clock, varying
    /// with the display's rate and with whatever else the machine is doing. It
    /// is here because smoothing a camera wants one and nothing downstream of
    /// it is hashed.
    ///
    /// # The obligation
    ///
    /// **Nothing this does may reach the simulation.** This runs a number of
    /// times between two ticks that depends on the display, on the window's
    /// state and on the machine's load, and it runs *zero* times in a headless
    /// run — so anything the simulation could read out of a controller would
    /// make the simulation a function of the display too.
    ///
    /// What that costs when it is got wrong is worth naming, because it is not
    /// what anyone expects. It is **not** primarily a desync: a controller is
    /// not hashed, so two peers whose controllers have diverged still compare
    /// digests and still agree — right up until the value crosses into a state,
    /// at which point the trace says the states diverged at tick N and says
    /// nothing about the display frame that caused it. It **is** a save that
    /// reloads into a different game, because what a session writes down is the
    /// state. And it **is** a rollback that does not roll back, because there
    /// is no controller to restore.
    fn update(&mut self, updating: Updating<'_, S>);

    /// Where this player is looking.
    ///
    /// A pure read of whatever [`update`](Self::update) computed. Asked once
    /// per displayed frame and handed to both the renderer and the ear, so that
    /// the eye and the ears are in the same place without either being told
    /// twice.
    fn look(&self) -> Camera;

    /// One tick's intent. **This is the whole of what goes on the wire.**
    ///
    /// `input` already carries `pressed` and `released` folded across every
    /// display frame since the last tick, the analog deflections, the mouse's
    /// displacement, the `Option<FineTransform>` poses a headset reports,
    /// and [`text`](corvid_input::Input::text) — so a tap that started and
    /// finished between two ticks is here rather than lost.
    ///
    /// # A controller the display never touched
    ///
    /// [`update`](Self::update) is what moves a camera, so a client whose
    /// window is minimised — and a headless run with no display at all — calls
    /// this against a controller that is still exactly as `new` left it. That
    /// is not an error and the runtime cannot make it not happen. It does mean
    /// an `action` that reads a smoothed camera is reading a number produced at
    /// display rate, so the actions a headless run submits are not the actions
    /// the same inputs produce in front of a player.
    fn action(&self, acting: Acting<'_, S>) -> S::Action;

    /// What this machine's pad should be doing, once per tick.
    ///
    /// Extracted from the state the way sound is: the tick records that a thing
    /// worth feeling happened, and this decides what the pad does about it.
    ///
    /// # Why this is not a `Command`
    ///
    /// Because it is one machine's hardware and nobody else's. Routing a haptic
    /// through a deterministic tick would put it on the wire and behind a
    /// network round trip, in order to reach a device exactly one peer has.
    /// That is the same argument the camera and the pointer are client-local
    /// for, and it has the same answer.
    ///
    /// Once per **tick** rather than once per frame, so an effect fires exactly
    /// once for the tick that earned it and there is no retrigger to
    /// deduplicate.
    fn rumble(&self, _acting: Acting<'_, S>) -> Option<RumbleId> {
        None
    }

    /// What the mouse pointer should be doing.
    ///
    /// Asked once per displayed frame, and answered from this controller —
    /// because whether the pointer is captured is a property of what the player
    /// is looking at. A game in its menu answers [`Cursor::Free`], the same
    /// game in play answers [`Cursor::Locked`].
    ///
    /// # It is a request
    ///
    /// Pointer locking is a permission in a browser, a protocol extension on
    /// Wayland, and a compositor's choice elsewhere. The runtime walks
    /// [`Cursor::fallback`] rather than failing — a refused
    /// [`Cursor::Locked`] becomes [`Cursor::Confined`] — and reports what
    /// actually happened through [`Input::cursor`], which the next frame's
    /// [`update`](Self::update) reads. A controller that assumes the lock took
    /// and steers from [`Input::pointer`] has a camera that stops at the edge
    /// of the monitor.
    fn cursor(&self) -> Cursor {
        Cursor::Free
    }

    /// Whether the simulation should advance. Client-local, like the camera.
    ///
    /// Asked once per reading of the clock, before any tick that reading owes.
    /// [`false`] is a pause: no tick runs, and [`update`](Self::update),
    /// [`look`](Self::look), `draw` and `hear` carry on exactly as they were —
    /// which is what a pause screen needs, since it has to be drawn and
    /// navigated while the world behind it holds still.
    ///
    /// The runtime does not advance its fixed step while this is false, so the
    /// real time that passed is discarded rather than accumulated: a pause of
    /// ten minutes is followed by one ordinary tick and not by nine thousand
    /// catch-up ticks.
    ///
    /// # This is a *client-local* pause
    ///
    /// Which is right for one machine and is not what a networked session does.
    /// No peer has this controller and no peer ever will, so a peer that stops
    /// ticking because somebody opened a menu is a peer that has fallen behind
    /// the session, not a session that has stopped. In a lockstep session a
    /// pause every peer agrees on is an
    /// [`Action`](corvid_behavior::State::Action), folded into the state the
    /// same way on every machine, and one player's menu cannot stop the others.
    #[must_use]
    fn simulating(&self) -> bool {
        true
    }
}

/// Nobody at the controls.
///
/// The default for an [`App`](../corvid_app/struct.App.html)'s controller, and
/// what a dedicated server has: it declares no actions, wants no devices,
/// submits the idle action forever and looks at the origin.
///
/// A dropped player submits `Action::default()` for ever, which is exactly what
/// this does — so a seat driven by this is a seat nobody is sitting in, and the
/// simulation already knows what to do with one.
impl<S: State> Controller<S> for () {
    type Config = ();

    const REAL: bool = false;
    const SETS: &'static [SetDescriptor] = &[];

    fn new((): ()) -> Self {}

    fn configure(&mut self, (): ()) {}

    fn update(&mut self, _updating: Updating<'_, S>) {}

    fn look(&self) -> Camera {
        Camera::default()
    }

    fn action(&self, acting: Acting<'_, S>) -> S::Action {
        let _ = acting;
        S::Action::default()
    }
}
