//! What a divergence is, and what else can go wrong on the way to finding one.

use corvid_app::Request;
use corvid_behavior::PlayerId;
use corvid_hash::Digest;
use corvid_replay::Load;
use corvid_time::Tick;

/// Two things that were meant to be the same session are not, and this is where
/// they stop being it.
///
/// The tick is the point. An assertion that says "the traces differ" costs
/// whoever reads it the whole debugging session: a simulation is a chain, so
/// everything after the first disagreement disagrees too, and the only tick with
/// any information in it is the first one. So this names that tick, the last
/// tick the two still agreed about, and what differs at the boundary.
///
/// # Which side is which
///
/// Every [`What`] below names two sides. The `recorded` one is the side that
/// already existed -- the first of two runs, or the marks a session carries. The
/// `computed` one is the side the check produced -- the second run, or the
/// replay. Neither is the right one: being written down first is not evidence
/// of anything.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("diverged at tick {at}, {}: {what}", reached(agreed_through.as_ref()))]
pub struct Diverged {
    /// The last tick the two agreed about, or [`None`] when they disagree at
    /// the first tick compared.
    ///
    /// As every check in this crate is written today it is `at.prev()` whenever
    /// there is one, because all of them walk forward from the session's
    /// opening. It is a field rather than something a reader derives because
    /// that is a property of the checks and not of the type: a comparison over
    /// an overlap that starts later -- two peers whose traces begin at different
    /// ticks, which
    /// [`HashTrace::disagrees_with`](corvid_replay::HashTrace::disagrees_with)
    /// already allows -- has a smaller answer, and how far a check got is
    /// something to be told rather than to infer from where it stopped.
    pub agreed_through: Option<Tick>,
    /// The first tick they disagree at.
    pub at: Tick,
    /// What differs there.
    pub what: What,
}

impl Diverged {
    /// A divergence at `at`, reached by walking from `first`.
    pub(crate) fn walked(first: Tick, at: Tick, what: What) -> Self {
        Self {
            agreed_through: (at > first).then(|| at.prev()),
            at,
            what,
        }
    }
}

/// How far the two got before they stopped agreeing, as a clause.
///
/// A function rather than a second [`Display`](core::fmt::Display) impl,
/// because the two readings are one sentence with one word different and the
/// alternative is a wrapper type nobody would name.
fn reached(agreed: Option<&Tick>) -> String {
    agreed.map_or_else(
        || "which is the first tick compared".to_owned(),
        |through| format!("agreed through {through}"),
    )
}

/// The ways two runs of one session stop being one session.
///
/// Every case is something a check compares directly. There is no "something
/// differs" case, because a message that says that is a message that names
/// nothing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum What {
    /// The two marked the same tick with different digests, which is the
    /// ordinary shape of a divergence: two states that are not the same state.
    #[error(
        "the state was marked {recorded} on the recorded side and {computed} on the computed one"
    )]
    Marks {
        /// What the side that already existed marked it.
        recorded: Digest,
        /// What the side produced by the check marked it.
        computed: Digest,
    },
    /// The two recorded different actions for one seat on this tick.
    ///
    /// A state divergence caused by an action divergence reports as
    /// [`Marks`](Self::Marks) instead, because the states are compared first.
    /// What reaches here is an action that differs *without* moving the state --
    /// an [`action`](corvid_control::Controller::action) that read something it
    /// should not have, on a tick where reading it happened not to matter.
    #[error("seat {} submitted {recorded} on the recorded side and {computed} on the computed one", seat.0)]
    Actions {
        /// Whose seat.
        seat: PlayerId,
        /// What the side that already existed submitted, as
        /// [`Debug`](core::fmt::Debug) renders the log's answer -- so `None` is
        /// a seat the log has no entry for rather than an action.
        ///
        /// A string because this type does not know the game's `Action`, and
        /// because a reader needs the value rather than the fact that it moved.
        recorded: String,
        /// What the other side submitted, likewise.
        computed: String,
    },
    /// The two did not get as far as each other, and `at` is the first tick one
    /// of them has no state for.
    ///
    /// The only way a run stops early is
    /// [`quit`](corvid_behavior::Command::quit), so
    /// this always arrives with a [`Requested`](Self::Requested) difference
    /// behind it. It is reported first anyway: two runs that stopped at
    /// different ticks are two different runs, and their request lists are not
    /// two lists of the same thing.
    #[error(
        "the recorded side reached tick {recorded} and the computed one reached tick {computed}, \
         so one of them stopped early -- look at what the ticks asked for"
    )]
    Reach {
        /// The last tick the side that already existed reached.
        recorded: Tick,
        /// The last tick the other side reached.
        computed: Tick,
    },
    /// The two asked the platform for different things.
    ///
    /// [`None`] on one side is a run that made no request where the other made
    /// one. A replay that re-issues a request saves a file, takes a screenshot
    /// or rumbles a controller for a second time, which is why the requests are
    /// compared at all rather than only the states.
    #[error(
        "the recorded side asked for {} and the computed one for {}",
        asked(recorded.as_deref()),
        asked(computed.as_deref())
    )]
    Requested {
        /// What the side that already existed asked for.
        recorded: Option<Box<Request>>,
        /// What the other side asked for.
        computed: Option<Box<Request>>,
    },
    /// The two states digest alike and compare unequal.
    ///
    /// Not a divergence between the two runs so much as one inside the game: its
    /// [`Eq`] can see a field its [`Hash`](core::hash::Hash) does
    /// not hash. Every other check in this crate compares digests, so this is
    /// the one that can see past them -- and a field outside the digest is a
    /// desync waiting for two peers to agree about a state they computed
    /// differently.
    #[error(
        "the two states compare unequal and digest alike ({digest}): this game's Eq and Hash \
         disagree, and the digest is the one that cannot tell them apart"
    )]
    Unequal {
        /// The digest both sides produced.
        digest: Digest,
    },
}

/// One side's request, or the fact that it made none.
///
/// Total where a match on the pair was not: a difference with neither side
/// present is not a difference, and spelling that case out meant carrying an
/// arm that said so.
fn asked(request: Option<&Request>) -> String {
    request.map_or_else(
        || "nothing more".to_owned(),
        |request| format!("{:?}", request.command),
    )
}

/// A check did not get as far as an answer, or got one.
///
/// The two halves are kept apart on purpose. A run that could not start and a
/// run that started twice and produced two different games are different
/// findings, and folding the first into [`Diverged`] would make an empty roster
/// read as nondeterminism.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Failed {
    /// The run could not be played. Nothing was compared.
    #[error("the run did not start: {0}")]
    Refused(#[source] corvid_app::Error),
    /// The session could not be written down. Nothing was compared.
    #[error("the session could not be written down: {0}")]
    Wrote(#[source] corvid_wire::Error),
    /// What was written down did not read back as a session. Nothing was
    /// compared.
    #[error("what was written down did not read back: {0}")]
    Read(#[source] Load),
    /// The comparison was made, and this is what it found.
    #[error(transparent)]
    Diverged(#[from] Diverged),
}
