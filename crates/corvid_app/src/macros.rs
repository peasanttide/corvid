//! The two macros a game's `main` is written with.
//!
//! [`game!`](crate::game) writes a marker and the [`Game`](crate::Game) it
//! implements; [`app!`](crate::app) writes that and the `main` that plays it.
//! Neither generates anything a game could not have written by hand — what they
//! remove is the five `type` lines, the derive the two `#[derive(Debug)]`s on
//! [`App`](crate::App) and [`Settings`](crate::Settings) ask for, and the
//! `fn main` that is the same three tokens in every Corvid binary.
//!
//! Everything they emit is reached through `$crate`, so a caller imports
//! nothing: a game that names only the `corvid` facade writes `corvid::app!`
//! and no `use` line for anything the expansion mentions.

/// Declares a game: a marker, and the [`Game`](crate::Game) it implements.
///
/// `struct` and `const PERIOD` are required. Every `type` line is optional and
/// defaults to `()` — which reads no device, runs no bot, draws nothing and
/// hears nothing.
///
/// # The order is fixed
///
/// The `type` lines are written as `State`, `Controller`, `Bot`, `Render`,
/// `Auralizer`, which is the order [`Game`](crate::Game) declares them in. Any
/// of them may be left out; none of them may be moved. This is a grammar rather
/// than a convention, and a line out of place is `no rules expected this token`
/// pointing at the line rather than at the order.
///
/// # The visibility, and the attributes
///
/// Both are the caller's. **The default is private**, which is what a game's
/// own `main.rs` wants — nothing else in a binary names the marker. A library
/// that exports its game writes `pub struct`, and a marker in a private module
/// that a test binary reaches writes `pub(crate)`.
///
/// Attributes written above the `struct` are forwarded, so a doc comment lands
/// under the generated first line as a second paragraph. A caller may add
/// derives the macro does **not** already emit; the list it does emit is under
/// *What it writes* below, and asking for one of those again is
/// `conflicting implementations`.
///
/// # What it writes
///
/// A unit struct deriving `Clone`, `Copy`, `Debug`, `Default`, `PartialEq`,
/// `Eq` and `Hash` — which is what [`App`](crate::App) and
/// [`Settings`](crate::Settings) ask of a `G`, since a derive bounds the type
/// parameter rather than the fields — the `impl Game`, and an
/// `app()` that builds the [`sandbox`](crate::App::sandbox) a test runs from.
///
/// # What the game owes
///
/// `app()` is written without a `where` clause of its own, so it is checked
/// against [`sandbox`](crate::App::sandbox) where it is defined rather than
/// where it is called: **all four configs must be [`Default`]** — the
/// controller's, the bot's, the renderer's and the ear's — or the game does not
/// compile at all, with the error pointing into this expansion. That is the
/// same clause [`App::new`](crate::App::new) carries, so it is a requirement a
/// game that could start a run already meets; what is new is that it is
/// reported here.
///
/// ```
/// # use std::sync::Arc;
/// # use corvid_app::App;
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Nowhere;
/// # impl corvid_behavior::Level for Nowhere {
/// #     type Error = core::convert::Infallible;
/// #     fn load(_: &str) -> Result<Self, core::convert::Infallible> { Ok(Self) }
/// # }
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Tally;
/// # impl corvid_behavior::State for Tally {
/// #     const NAME: &'static str = "tally";
/// #     type Level = Nowhere; type Rules = (); type Action = ();
/// # }
/// # impl corvid_replay::Opens for Tally {
/// #     fn opening() -> corvid_replay::Opening<Self> {
/// #         corvid_replay::Opening {
/// #             level: String::new(),
/// #             content: Arc::new(Nowhere),
/// #             rules: Arc::new(()),
/// #             roster: vec![corvid_replay::Profile {
/// #                 account: corvid_behavior::ProfileId(1),
/// #                 joined: corvid_time::Tick::ZERO,
/// #                 left: None,
/// #             }],
/// #             seed: corvid_replay::Seed(0),
/// #             first: corvid_time::Tick::ZERO,
/// #             origin: None,
/// #             schema: corvid_replay::Schema::new("tally").digest(),
/// #         }
/// #     }
/// # }
/// use corvid_time::TickSpan;
///
/// corvid_app::game! {
///     struct Counting;
///     const PERIOD: TickSpan = TickSpan::from_millis(66);
///     type State = Tally;
/// }
///
/// // Everything unnamed is `()`.
/// assert_eq!(<Counting as corvid_app::Game>::PERIOD, TickSpan::from_millis(66));
/// let _: App<Counting> = Counting::app();
/// ```
#[macro_export]
macro_rules! game {
    // The internal rules come first so that a real invocation, which begins
    // with attributes and a visibility that both match nothing, is never
    // matched against a `@` it cannot reach.
    (@or_unit) => { () };
    (@or_unit $type:ty) => { $type };
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $name:ident;
        const PERIOD: $span:ty = $period:expr;
        $(type State = $state:ty;)?
        $(type Controller = $controller:ty;)?
        $(type Bot = $bot:ty;)?
        $(type Render = $render:ty;)?
        $(type Auralizer = $auralizer:ty;)?
    ) => {
        #[doc = concat!(
            "The `", stringify!($name), "` game: five types, and how long its tick lasts."
        )]
        #[doc = ""]
        $(#[$attribute])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        $visibility struct $name;

        impl $crate::Game for $name {
            const PERIOD: $span = $period;

            type State = $crate::game!(@or_unit $($state)?);
            type Controller = $crate::game!(@or_unit $($controller)?);
            type Bot = $crate::game!(@or_unit $($bot)?);
            type Render = $crate::game!(@or_unit $($render)?);
            type Auralizer = $crate::game!(@or_unit $($auralizer)?);
        }

        impl $name {
            /// A headless run with a scratch state directory and no settings
            /// file read.
            ///
            /// What a test wants: nothing about it depends on the machine it
            /// runs on, and one call stands for the builder lines every test
            /// file would otherwise repeat.
            #[allow(
                dead_code,
                reason = "a game declared here is a declaration of its types; whether anything in this crate builds a sandbox from it is a separate question, and a binary that only plays the game never asks"
            )]
            #[must_use]
            $visibility fn app() -> $crate::App<Self> {
                $crate::App::<Self>::sandbox()
            }
        }
    };
}

/// Declares a game and the `main` that plays it.
///
/// The whole of a Corvid binary. Everything [`game!`](crate::game) accepts,
/// plus a `main` that reads the command line and decides the shape of the run —
/// which is [`main`](crate::main), and is the same three tokens in every game
/// that has one.
///
/// ```no_run
/// # use std::sync::Arc;
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Nowhere;
/// # impl corvid_behavior::Level for Nowhere {
/// #     type Error = core::convert::Infallible;
/// #     fn load(_: &str) -> Result<Self, core::convert::Infallible> { Ok(Self) }
/// # }
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Bounce;
/// # impl corvid_behavior::State for Bounce {
/// #     const NAME: &'static str = "bounce";
/// #     type Level = Nowhere; type Rules = (); type Action = ();
/// # }
/// # impl corvid_replay::Opens for Bounce {
/// #     fn opening() -> corvid_replay::Opening<Self> {
/// #         corvid_replay::Opening {
/// #             level: String::new(),
/// #             content: Arc::new(Nowhere),
/// #             rules: Arc::new(()),
/// #             roster: Vec::new(),
/// #             seed: corvid_replay::Seed(0),
/// #             first: corvid_time::Tick::ZERO,
/// #             origin: None,
/// #             schema: corvid_replay::Schema::new("bounce").digest(),
/// #         }
/// #     }
/// # }
/// use corvid_time::TickSpan;
///
/// corvid_app::app! {
///     struct Hello;
///     const PERIOD: TickSpan = TickSpan::CRADLE;
///     type State = Bounce;
/// }
/// ```
#[macro_export]
macro_rules! app {
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $name:ident;
        $($rest:tt)*
    ) => {
        $crate::game! {
            $(#[$attribute])*
            $visibility struct $name;
            $($rest)*
        }

        fn main() {
            $crate::main::<$name>()
        }
    };
}
