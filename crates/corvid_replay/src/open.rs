//! Where a run that has nothing to resume from starts.

use corvid_behavior::State;

use crate::Opening;

/// The session a game starts a fresh run on.
///
/// [`Opening`] is the one thing a runtime cannot invent: a level, a set of
/// rules, a roster and an origin state are the game's, and no crate that does
/// not know the game can produce them. This is how a game states them once, so
/// that starting a run means naming the game and nothing else.
///
/// It is a trait rather than an argument because the entry point takes no
/// arguments — `corvid_app::main::<G>()` reads the process's command line and
/// decides everything else — and it lives here rather than on
/// [`State`] because [`Opening`] is this crate's type and the simulation
/// ring does not depend on this crate.
///
/// A run that was given `--load` or `--replay` never calls this: a saved
/// session and a recorded one both carry their own opening, and this is what a
/// run with neither reaches for.
///
/// ```
/// use std::sync::Arc;
///
/// use corvid_replay::{Opening, Opens, Profile, Schema, Seed, Session};
/// use corvid_time::Tick;
/// # use corvid_behavior::{Level, ProfileId, State};
/// # use corvid_files::{Malformed, Source};
/// # use serde::{Deserialize, Serialize};
///
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Only;
/// # impl Level for Only {
/// #     type Reference = String;
/// #     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
/// # }
/// #
/// #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// struct Counter(i64);
/// # impl State for Counter {
/// #     const NAME: &'static str = "counter";
/// #     type Level = Only;
/// #     type Rules = ();
/// #     type Action = ();
/// # }
///
/// impl Opens for Counter {
///     fn opening() -> Opening<Self> {
///         Opening {
///             level: "the only one".to_owned(),
///             content: Arc::new(Only),
///             rules: Arc::new(()),
///             roster: vec![Profile {
///                 account: ProfileId(1),
///                 joined: Tick::ZERO,
///                 left: None,
///             }],
///             seed: Seed(0),
///             first: Tick::ZERO,
///             // `None` is `Counter::default()`, which is what a fresh
///             // session opens on and what most games want here.
///             origin: None,
///             schema: Schema::new("counter").field("State", "i64").digest(),
///         }
///     }
/// }
///
/// // Which is everything a session needs.
/// let session = Session::new(Counter::opening())?;
/// assert_eq!(session.first(), Tick::ZERO);
/// # Ok::<(), corvid_replay::Shape>(())
/// ```
pub trait Opens: State + Sized {
    /// The session a fresh run plays from its first tick.
    ///
    /// Called once, at start-up, and never during a run.
    fn opening() -> Opening<Self>;
}
