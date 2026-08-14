//! How much of the session a run keeps while it is running.

/// How much of its own history a run holds on to.
///
/// A run writes a row of actions and a digest every tick and, if it never seeks,
/// reads neither again. An hour at [`TickSpan::CRADLE`](corvid_time::TickSpan)
/// is 54 000 of each -- fifteen a second, sixty minutes -- and a game left running
/// over lunch was keeping every one of them for nobody.
///
/// So the default is bounded and keeping everything is something a run asks for.
/// What "bounded" costs is *reach*: save, replay, rollback and time-walk are the
/// same [`seek`](corvid_replay::Session::seek) over whatever the session still
/// holds, so all four still work and none of them reaches further back than the
/// window. The crate documentation has the table of what a default run can and
/// cannot do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Retention {
    /// Every tick from the opening, which is what a run being recorded wants and
    /// what grows without bound.
    Everything,
    /// The recent past, and enough of it for a rollback to have somewhere to
    /// land.
    ///
    /// The run holds **at least** `ticks` ticks -- once it has played that many
    /// -- and never more than twice that. A run shorter than its own window has
    /// forgotten nothing and holds exactly what it played, which is less than
    /// `ticks` and is the whole of it; the floor is a promise about reach and
    /// not about size. It is a range rather than a number because of what the
    /// tight version would cost: forgetting the row at exactly `now - ticks` on every tick
    /// means having the state at `now - ticks` on every tick, which is a ring of
    /// whole states rather than a window of actions. Instead the run keeps one
    /// state aside every `ticks` ticks and forgets back to the previous one, so
    /// the price is a single [`Clone`] of the state per window and the history
    /// sawtooths between one window and two.
    ///
    /// `ticks: 0` is legal and leaves the run holding the one row it has just
    /// written and nothing before it, at the cost of a state clone every tick.
    /// It is the floor rather than a useful setting: a session always covers the
    /// tick it is on, because that is where the loop is writing.
    Recent {
        /// How far back the run is sure to be able to reach.
        ticks: u64,
    },
}

impl Retention {
    /// The window a run gets when nobody says otherwise.
    ///
    /// At [`TickSpan::CRADLE`](corvid_time::TickSpan) this is seventeen seconds
    /// of ticks and the sawtooth reaches thirty-four. That is chosen against
    /// what reaches backwards rather than against a memory figure: a rollback
    /// reaches back a network round trip, which is under a second; a desync
    /// bisection reaches back to the last agreed snapshot; and the time slider
    /// in a dev console reaches back as far as somebody remembers doing
    /// something they want to see again. Seventeen seconds is past all three
    /// with room, and a run that wants a session it can scrub to the beginning
    /// of is asking for a recording -- which is [`Everything`](Self::Everything),
    /// and is what [`capture`](crate::App::capture) already implies.
    pub const RECENT: Self = Self::Recent { ticks: 256 };
}

/// [`RECENT`](Self::RECENT), because a run nobody is recording is the common
/// case and the one that must not grow.
impl Default for Retention {
    fn default() -> Self {
        Self::RECENT
    }
}
