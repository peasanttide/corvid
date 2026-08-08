//! What a game is: five types and a tick span.

use corvid_behavior::State;
use corvid_control::Controller;
use corvid_render::Render;
use corvid_replay::Opens;
use corvid_sound::Auralizer;
use corvid_time::TickSpan;

/// The five types a game is, and how long its tick lasts.
///
/// A run names one of these instead of five parameters, which is what lets a
/// game's `main` be a declaration: everything the runtime needs to know about a
/// game is reachable from one name, so [`App`](crate::App),
/// [`Settings`](crate::Settings) and every type between them take one parameter
/// and agree about what it means.
///
/// It is implemented on a marker rather than on the state, because the state is
/// only one of the five and a game is the whole set. Two games can share a
/// [`State`] and differ in who is at the controls — a scripted run and a played
/// one are exactly that — and each of them is a `Game` of its own.
///
/// # The bot
///
/// [`Bot`](Self::Bot) is a second [`Controller`], and a game with no bots names
/// `()` — which declares no actions, wants no devices and submits the idle
/// action forever. One instance answers for every seat a run gives it, told
/// which by [`Acting::seat`](corvid_control::Acting).
///
/// # What a game owes, and what it does not
///
/// Nothing here has a default. Four of the five types have a `()` implementation
/// that costs nothing — no controller, no bot, no renderer, no ear — so a
/// dedicated server writes `()` four times and says so, rather than leaving four
/// blanks that a reader has to know the meaning of.
///
/// The implementing type is a marker with no fields, and it wants
/// `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`: the derives on
/// [`Settings`](crate::Settings) and [`App`](crate::App) ask for each of those
/// on `G` itself, which is what a derive does with a type parameter and is a
/// line rather than a cost on a type with nothing in it.
pub trait Game {
    /// How long one tick lasts. Every peer must agree.
    ///
    /// [`TickSpan::from_millis`] is what a game writes. It is a constant rather
    /// than a setting because it is a property of the *session*: two peers on
    /// different periods compute different states from the same actions, so
    /// this is not something one machine may decide for itself.
    ///
    /// [`App::rate`](crate::App::rate) defaults to it, and overriding it is a
    /// harness running a game fast rather than a game changing its mind.
    const PERIOD: TickSpan;

    /// The deterministic half, and where a session starts.
    ///
    /// [`Opens`] as well as [`State`], because a run that was given neither a
    /// save nor a recording has to start somewhere and no crate that does not
    /// know the game can invent an opening for it.
    type State: State + Opens;

    /// Who is at the controls.
    type Controller: Controller<Self::State>;

    /// What plays the seats nobody is in.
    type Bot: Controller<Self::State>;

    /// What draws.
    type Render: Render<Self::State>;

    /// What sounds.
    type Auralizer: Auralizer<Self::State>;
}

/// The controller's config, spelled once.
///
/// This and the three below are the only place in the workspace the
/// `<… as …>::Config` form is written. It is four lines of noise at every site
/// that names one — a `where` clause, a struct field, a builder argument — and
/// naming it here means a reader meets the projection once and reads
/// [`Settings`](crate::Settings)' fields as the four settings they are.
pub type ControllerConfig<G> = <<G as Game>::Controller as Controller<<G as Game>::State>>::Config;

/// The bot's config, spelled once.
pub type BotConfig<G> = <<G as Game>::Bot as Controller<<G as Game>::State>>::Config;

/// The renderer's config, spelled once.
pub type RenderConfig<G> = <<G as Game>::Render as Render<<G as Game>::State>>::Config;

/// The ear's config, spelled once.
pub type AuralizerConfig<G> = <<G as Game>::Auralizer as Auralizer<<G as Game>::State>>::Config;
