//! The seek, and the generation rule.

use alloc::sync::Arc;

use corvid_behavior::{Command, LevelEdit, State};
use corvid_replay::{Session, Stepped};
use corvid_time::Tick;

/// How far a rollback went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rolled {
    /// The tick the correction landed on. The state *at* this tick is the one
    /// the re-simulation started from, because it is what the rows before it
    /// produce and the correction is to the row *at* it.
    pub from: Tick,
    /// The tick the re-simulation reached.
    pub to: Tick,
    /// How many ticks were re-simulated.
    pub ticks: u8,
}

impl Rolled {
    /// Whether anything was re-simulated.
    #[must_use]
    pub const fn happened(&self) -> bool {
        self.ticks > 0
    }
}

/// What one call to [`Peer::advance`](crate::Peer::advance) did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Advanced {
    /// The tick the peer is on now.
    pub tick: Tick,
    /// How many seats that tick had to predict.
    pub predicted_seats: u16,
    /// Whether the peer is behind where it wants to be.
    ///
    /// Either it declined to simulate -- it is
    /// [`Budget::ahead`](crate::Budget::ahead) past
    /// [`Frontier::agreed`](crate::Frontier::agreed) and predicting further
    /// would be predicting a
    /// decision -- or it simulated one tick and is still short of the tick it
    /// was on before a rollback deeper than
    /// [`Budget::rollback`](crate::Budget::rollback) rewound it.
    ///
    /// Stalling is a decision and not a failure. A visible hitch is better than
    /// a missed frame budget, and this is what a lab draws.
    pub stalled: bool,
}

/// Simulates one tick against a row.
///
/// The roster is rebuilt from the opening exactly as
/// [`Session::seek`](corvid_replay::Session::seek) rebuilds it -- the seat is the
/// roster's order, the presence is
/// [`Profile::presence_at`](corvid_replay::Profile::presence_at), and the action
/// is the row's -- so a tick simulated here and the same tick reached by a seek
/// are handed the same arguments.
///
/// The sink is the caller's, and what decides
/// whether anybody acts on them is [`Peer::simulate_one`](crate::Peer): a tick
/// simulated for the first time asked for them, and the same tick re-simulated
/// by a rollback is asking again for something already asked.
pub(crate) fn step<S: State>(
    session: &Session<S>,
    level: &Arc<S::Level>,
    previous: &S,
    at: Tick,
    row: &[S::Action],
    command: &mut impl Command<LevelEdit<S>>,
) -> Stepped<S> {
    let players = corvid_replay::players(&session.opening, at, |seat| {
        row.get(usize::from(seat.0)).cloned()
    });
    corvid_replay::step(
        S::clone(previous),
        level,
        &players,
        &session.opening.rules,
        command,
    )
}
