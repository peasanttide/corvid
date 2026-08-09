//! Playing one opening twice and finding the first tick the two runs stop being
//! the same game.

use core::marker::PhantomData;

use corvid_app::{App, Game, Outcome, Retention, Settings};
use corvid_behavior::{PlayerId, State};
use corvid_hash::Digest;
use corvid_hash::digest;
use corvid_input::Input;
use corvid_replay::LevelRef;
use corvid_replay::{Opening, Opens};
use corvid_time::{Tick, TickSpan, Ticks};

use crate::{Diverged, Failed, What};

/// The game the two runs play: the caller's state and controller, and nothing
/// else.
///
/// [`App`] takes one parameter and that parameter is a whole game, so a check
/// about a *state* and a *controller* has to name a game to run one — and the
/// other three are decided rather than asked for. No bot, because a run with
/// one is a run whose seats are filled by something the caller did not hand
/// over; no renderer and no ear, because a determinism check that opened an
/// adapter or a sound card would be a determinism check with a side effect, and
/// neither is on the path from an action log to a state.
///
/// A private marker rather than a parameter on [`is_reproducible`], so that a
/// caller with a state and a controller still writes exactly those two. What it
/// cannot hide is [`Opens`], which a `Game`'s state owes and which therefore
/// reaches the caller's `where` clause even though nothing here calls it —
/// [`is_reproducible`] says so out loud.
struct Twice<S, C>(PhantomData<fn() -> (S, C)>);

impl<S: State + Opens, C: corvid_control::Controller<S>> Game for Twice<S, C> {
    /// The rate is not a variable here. Both runs use it, a stepping clock
    /// makes each reading one owed tick whatever it is, and nothing either run
    /// computes depends on it — so a period this check invented is a period
    /// neither comparison can see.
    const PERIOD: TickSpan = TickSpan::CRADLE;

    type State = S;
    type Controller = C;
    type Bot = ();
    type Render = ();
    type Auralizer = ();
}

/// Plays `opening` twice with the same input and compares the two runs.
///
/// This is the headline claim, as one call: the same inputs produce the same
/// state, bit for bit. Both runs go through [`App`], so what is compared is the
/// game as the runtime actually drives it — `action` every tick, `update` and
/// `look` every displayed frame — rather than a loop this crate wrote for the
/// occasion.
///
/// Five comparisons, in this order. Each is the first thing that would still be
/// true if the one before it were removed:
///
/// | | What it catches |
/// |---|---|
/// | the marks, over the ticks both reached | two states that are not the same state, which is what a divergence usually is |
/// | the actions, over the same ticks | an `action` that read something it should not have, on a tick where reading it did not move the state |
/// | how far each run reached | a [`quit`](corvid_behavior::Command::quit) asked for on different ticks |
/// | what the ticks asked the platform for | a save to a different slot, a screenshot on a different tick — a request that differs while the state does not |
/// | the two final states by [`Eq`] | the field a game's `Hash` does not absorb and its `Eq` does |
///
/// The reach is compared before the requests deliberately. A run only stops
/// early because a tick asked to quit, so a reach difference always has a
/// request difference behind it — but two runs that stopped at different ticks
/// are two different runs, and their request lists are not two lists of the same
/// thing.
///
/// # The bounds
///
/// Four, and the signature carries every one of them:
///
/// | | Why |
/// |---|---|
/// | `S: State` | what is being checked: a tick is a pure function of the values its arguments denote, and this plays two of them |
/// | `S: Opens` | propagated, and never called — see below |
/// | `C: Controller<S>` | who plays. The action a controller returns is the other half of what a run computes from, so a check that fixed the controller would be checking half a game |
/// | `C::Config: Clone + Default` | `Clone` because both runs are built from the same `controls`, and `Default` because the three types this check decides for itself still need configs to be built from |
///
/// [`Opens`] is the odd one, and it is worth being plain about it: **this
/// function never calls it.** The opening is a parameter, and it is what both
/// runs are given. The bound is there because [`App`] plays a
/// [`Game`](corvid_app::Game) and a `Game` names a `State` that can open a
/// session on its own. So a caller owes an `opening()` that nothing on this
/// path invokes — which is the cost of running the check through the same
/// runtime a player would, rather than through a loop this crate wrote for the
/// occasion.
///
/// A bot, a renderer and an ear are **not** bounds. This check decides all
/// three, as `()`: a determinism check that opened an adapter or a sound card
/// would be a determinism check with a side effect, and neither a picture nor a
/// sound is on the path from an action log to a state.
///
/// # What the type system enforces, and what the caller owes
///
/// Nothing here is enforced by a bound. [`State::tick`](corvid_behavior::State::tick)
/// is a free function with no `&self`, which narrows what a tick can reach for
/// and proves nothing, and the simulation ring is `no_std`, which takes away the
/// clock, the filesystem and the environment. What is left over is what this
/// checks for, and what it can check for is bounded by running in one process.
///
/// **Two runs in one process cannot see a constant.** A `static AtomicU64` set
/// at startup, a value derived from the machine's core count, a lazily
/// initialised table: both runs read the same number, and there is nothing here
/// to compare. What this can catch is a global that *moves* between the two
/// runs, which is the commoner shape of the same bug and is what the fixtures in
/// `tests/` are built out of. What catches a genuine constant is two peers that
/// are two processes, comparing digests, which nothing in this workspace does.
///
/// **It says nothing about a second machine.** Two runs on one machine share a
/// target, a compiler and a libm. Comparing recorded digests across targets is a
/// job for a CI matrix rather than for a function here.
///
/// **It plays the run it is given.** A leak that needs a joining player, a
/// second level or a particular set of rules is invisible in a session that has
/// none of them.
///
/// # Both runs keep everything
///
/// [`Retention::Everything`](corvid_app::Retention::Everything), said here
/// rather than inherited. A run's default is a window of its recent history,
/// and two runs that had both let go of their first thousand ticks would be
/// compared over what they still held — so a divergence older than the window
/// would be compared against nothing at all and reported as agreement. The
/// price is that a check of a very long run holds a row of actions and a digest
/// per tick for both of its runs, which is what `ticks` is for.
///
/// # `ticks`
///
/// How far each run plays. The runtime's stop predicate is checked after a tick
/// has run, so there is no way to ask it for none and a `ticks` of zero plays
/// one — which is harmless, because a comparison of two states cloned from one
/// opening has nothing in it either way. A run whose game asks to quit first
/// stops there instead, in both runs or in neither, and that difference is the
/// third comparison above.
///
/// # Errors
///
/// [`Failed::Refused`] if a run could not start — an opening with no seat zero,
/// most likely — and [`Failed::Diverged`] naming the first tick the two runs
/// disagree at.
pub fn is_reproducible<S, C>(
    opening: &Opening<S>,
    controls: &C::Config,
    input: &Input,
    ticks: u64,
) -> Result<(), Failed<LevelRef<S>>>
where
    S: State + Opens,
    C: corvid_control::Controller<S>,
    C::Config: Clone + Default,
{
    let recorded = play::<S, C>(opening.clone(), controls.clone(), input.clone(), ticks)?;
    let computed = play::<S, C>(opening.clone(), controls.clone(), input.clone(), ticks)?;
    compare(&recorded, &computed).map_err(Failed::Diverged)
}

/// One run of `ticks` ticks, keeping all of it.
fn play<S, C>(
    opening: Opening<S>,
    controls: C::Config,
    input: Input,
    ticks: u64,
) -> Result<Outcome<Twice<S, C>>, Failed<LevelRef<S>>>
where
    S: State + Opens,
    C: corvid_control::Controller<S>,
    C::Config: Default,
{
    // Counted rather than read off the state, because a `State` need not carry
    // its own tick number: `tick` is not handed one, and whether a game keeps
    // one is the game's business. `for_ticks` and not a counting predicate:
    // counting ticks in a closure here would make this check and the run its
    // goldens came from two hand-synchronised definitions of "how long" that
    // could drift apart.
    //
    // `retain` is said here rather than left at its default, and it is the one
    // setting on this builder that this check cannot afford to inherit. A run
    // nobody records keeps a window of its own history, so the comparisons
    // below would run over the ticks the two runs still held rather than over
    // the ticks they played — and a divergence at a tick that had scrolled out
    // of that window would be compared against nothing and reported as
    // agreement. `tests/reproducible.rs` has the case, at a tick chosen to be
    // outside the default window.
    App::<Twice<S, C>>::new()
        .headless()
        .opening(opening)
        .settings(Settings {
            controls,
            ..Settings::default()
        })
        .input(input)
        .retain(Retention::Everything)
        .for_ticks(Ticks(ticks.max(1)))
        .run()
        .map_err(Failed::Refused)
}

/// The five comparisons, in the order the documentation gives.
fn compare<G: Game>(
    recorded: &Outcome<G>,
    computed: &Outcome<G>,
) -> Result<(), Diverged<LevelRef<G::State>>> {
    let first = recorded.session.first();

    if let Some(at) = recorded
        .session
        .marks
        .disagrees_with(&computed.session.marks)
    {
        return Err(Diverged::walked(
            first,
            at,
            What::Marks {
                recorded: recorded.session.marks.get(at).unwrap_or(Digest::ZERO),
                computed: computed.session.marks.get(at).unwrap_or(Digest::ZERO),
            },
        ));
    }

    if let Some(diverged) = actions(recorded, computed) {
        return Err(diverged);
    }

    let (reached, also) = (recorded.session.last(), computed.session.last());
    if reached != also {
        let ends = reached.min(also);
        return Err(Diverged {
            agreed_through: Some(ends),
            at: ends.next(),
            what: What::Reach {
                recorded: reached,
                computed: also,
            },
        });
    }

    if let Some(diverged) = requests(recorded, computed, reached) {
        return Err(diverged);
    }

    // Every digest agreed — the last mark is the digest of the last state — so
    // what is left is a difference the digest cannot see.
    if recorded.state != computed.state {
        return Err(Diverged {
            agreed_through: (reached > first).then(|| reached.prev()),
            at: reached,
            what: What::Unequal {
                digest: digest(&recorded.state),
            },
        });
    }
    Ok(())
}

/// The first tick and seat the two logs disagree about, over the ticks both
/// runs reached.
fn actions<G: Game>(
    recorded: &Outcome<G>,
    computed: &Outcome<G>,
) -> Option<Diverged<LevelRef<G::State>>> {
    let first = recorded.session.first().max(computed.session.first());
    let until = recorded.session.last().min(computed.session.last());
    // The narrower of the two, so that a roster this crate cannot produce — the
    // two openings are clones of one value — is compared over what both have
    // rather than indexed past the end of one.
    let seats = recorded
        .session
        .log
        .players()
        .min(computed.session.log.players());

    let mut at = first;
    while at < until {
        for index in 0..seats {
            let seat = PlayerId(index);
            let mine = recorded.session.log.get(at, seat);
            let theirs = computed.session.log.get(at, seat);
            if mine != theirs {
                return Some(Diverged::walked(
                    recorded.session.first(),
                    at,
                    What::Actions {
                        seat,
                        recorded: format!("{mine:?}"),
                        computed: format!("{theirs:?}"),
                    },
                ));
            }
        }
        at = at.next();
    }
    None
}

/// The first request the two runs disagree about.
///
/// Compared by position rather than by tick: a run that asked for two things on
/// one tick and a run that asked for one are different at the second, and a
/// comparison keyed on the tick would have to decide which of the two to hold
/// against it.
fn requests<G: Game>(
    recorded: &Outcome<G>,
    computed: &Outcome<G>,
    last: Tick,
) -> Option<Diverged<LevelRef<G::State>>> {
    let mut mine = recorded.requests.iter();
    let mut theirs = computed.requests.iter();
    loop {
        let (one, two) = match (mine.next(), theirs.next()) {
            // Both lists ran out together, which is the only way they end
            // without differing.
            (None, None) => return None,
            (one, two) if one == two => continue,
            differ => differ,
        };
        // Whichever side has a request names the tick, preferring the recorded
        // one because that is the run a reader has a capture of. `last` is the
        // fallback for two absent requests, which the arm above has already
        // taken as the end of both lists — it is here because this crate does
        // not write `unreachable`, and a tick number that is merely unhelpful
        // beats one that cannot be printed.
        let at = one.or(two).map_or(last, |request| request.tick);
        return Some(Diverged::walked(
            recorded.session.first(),
            at,
            What::Requested {
                recorded: one.cloned().map(Box::new),
                computed: two.cloned().map(Box::new),
            },
        ));
    }
}
