//! Prediction, and what a mispredict is.

use alloc::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_replay::{ActionLog, Refused};
use corvid_time::Tick;

use crate::{Datagram, Frontier};

/// How much of a row was predicted rather than confirmed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Predicted {
    /// How many seats were predicted.
    pub seats: u16,
    /// How many of those had never acted, so their prediction is
    /// `Action::default()` rather than a repeat of anything.
    pub from_default: u16,
}

/// What a real action did to a prediction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Correction {
    /// It matched. The log is confirmed and nothing is re-simulated.
    Agreed,
    /// It did not. Everything strictly after this tick is stale.
    Mispredicted {
        /// The tick that was mispredicted, not the tick after.
        at: Tick,
    },
    /// It was for a tick already confirmed, and identical. A duplicate.
    Duplicate,
    /// It was for a tick already confirmed, and different.
    ///
    /// A peer that sends action X for tick 40 and later sends action Y for tick
    /// 40 is not a network problem -- it is a peer whose log has diverged from
    /// everyone else's, and continuing would mean picking one of two stories.
    /// It halts the session the same way a desync does, and the report says
    /// which.
    Contradiction {
        /// The tick the two stories disagree about.
        at: Tick,
    },
}

/// The action `seat` is simulated with at `at`.
///
/// The confirmed one where the seat has confirmed that tick, and otherwise that
/// seat's newest confirmed action at or before it -- the repeat that prediction
/// is made of. [`None`] for a seat with nothing to repeat, whose prediction is
/// `Action::default()`.
///
/// Right almost always for a tower defence: a player who was idle stays idle,
/// and a player mid-build stays mid-build. The rollback that matters is the one
/// caused by a decision, and a decision is rare.
///
/// ```
/// # use corvid_behavior::PlayerId;
/// # use corvid_lockstep::{Frontier, action_at};
/// # use corvid_time::Tick;
/// # use corvid_replay::ActionLog;
/// let mut log = ActionLog::<u8>::new(Tick::ZERO, 1);
/// let mut frontier = Frontier::new(1);
/// log.extend_to(Tick(8))?;
/// log.set(Tick(5), PlayerId(0), 7)?;
/// frontier.observe(PlayerId(0), Tick(5));
///
/// // Absent at 6, 7 and 8, so tick 5's action is what those three run.
/// for tick in [6, 7, 8] {
///     assert_eq!(action_at(&log, &frontier, Tick(tick), PlayerId(0)), Some(&7));
/// }
/// # Ok::<(), corvid_replay::Refused>(())
/// ```
#[must_use]
pub fn action_at<'log, A>(
    log: &'log ActionLog<A>,
    frontier: &Frontier,
    at: Tick,
    seat: PlayerId,
) -> Option<&'log A> {
    if log.is_confirmed(at, seat) {
        return log.get(at, seat);
    }
    // A seat whose machine has gone submits `Action::default()` for ever, which
    // is what `Presence::Dropped` means -- so there is nothing to repeat even
    // though there is something to repeat *from*.
    if frontier.is_retired(seat) {
        return None;
    }
    // Nothing to repeat. Walking backwards for a seat that has never acted
    // would be a scan of the whole log to reach the same answer.
    let confirmed = frontier.confirmed(seat)?;
    let first = log.first();
    // The newest confirmed row at or before `at`. Usually the one just before
    // it, because a datagram carries four rows and a seat's confirmations are
    // therefore contiguous unless four in a row were lost.
    let mut probe = if confirmed < at { confirmed } else { at.prev() };
    loop {
        if probe < first {
            return None;
        }
        if log.is_confirmed(probe, seat) {
            return log.get(probe, seat);
        }
        if probe == first {
            return None;
        }
        probe = probe.prev();
    }
}

/// The whole row `at` is simulated with, seat by seat.
///
/// Confirmed where a seat has confirmed it and predicted where it has not, with
/// `A::default()` for a seat with nothing to repeat. `out` is cleared first, so
/// one buffer serves every tick.
pub fn row_at<A: Clone + Default>(
    log: &ActionLog<A>,
    frontier: &Frontier,
    at: Tick,
    out: &mut Vec<A>,
) {
    out.clear();
    for seat in 0..log.players() {
        let action = action_at(log, frontier, at, PlayerId(seat));
        out.push(action.cloned().unwrap_or_default());
    }
}

/// Makes a row for `at` and says how much of it is a prediction.
///
/// The prediction itself is not written down. An
/// [`ActionLog`] entry is either confirmed by the seat that sent it or it is
/// not, and a prediction is neither -- it is
/// [`action_at`] of the log and the frontier, which is a function of what the
/// log already holds. Writing it in would make the next real action a
/// contradiction rather than a correction, because the log would already be
/// confirming a value nobody sent.
///
/// What this does write is the row itself: the log is grown so that `at` has
/// one, which is the decision a runtime makes explicitly at a call site that
/// knows how far ahead of the present it is willing to record.
///
/// # Errors
///
/// [`Refused`], from [`ActionLog::extend_to`], for a tick before the log's
/// first or one this machine could not find the memory for.
pub fn predict<A: Clone + Default + PartialEq>(
    log: &mut ActionLog<A>,
    frontier: &Frontier,
    at: Tick,
) -> Result<Predicted, Refused> {
    log.extend_to(at)?;
    let mut predicted = Predicted::default();
    for seat in 0..log.players() {
        let seat = PlayerId(seat);
        if log.is_confirmed(at, seat) {
            continue;
        }
        predicted.seats = predicted.seats.saturating_add(1);
        if action_at(log, frontier, at, seat).is_none() {
            predicted.from_default = predicted.from_default.saturating_add(1);
        }
    }
    Ok(predicted)
}

/// Folds one arrived datagram into the log.
///
/// The rows are taken oldest first, so each one's prediction is judged against
/// the log as the rows before it left it. Where one datagram names a tick more
/// than once -- which the ones sent in a session's first three ticks do, because
/// [`Datagram::ticks`] saturates at the opening rather than naming ticks that
/// never ran -- the last slot named for that tick is the one that came out of
/// the sender's log, and it is the one that counts.
///
/// The answer is the worst thing that happened to any row: a contradiction
/// outranks a mispredict, a mispredict names the oldest tick it happened on,
/// and a datagram whose every row was already confirmed and identical is a
/// [`Duplicate`](Correction::Duplicate).
///
/// # Rows outside the log are skipped rather than refused
///
/// A datagram carries a window, and a peer that is behind the sender receives
/// windows whose newest rows are for ticks it has not made room for. Those rows
/// are ignored and the rest are folded in, which is what makes a peer that
/// starts late -- or a thread that got less of the processor for a moment --
/// catch up rather than fail: the rows it skipped are still in the sender's
/// window until it acknowledges them.
///
/// How far ahead the log has room for is
/// [`Budget::horizon`](crate::Budget::horizon)'s decision and
/// [`Peer::receive`](crate::Peer::receive) is where it is made. That is the
/// denial-of-service arm and it is unchanged: a `Tick` is the one number in a
/// session that arrives from somewhere else, and nothing here grows a log to
/// reach one. **What changed is that a tick past the horizon no longer stops
/// the session** -- a stranger with a socket could otherwise end a game by
/// sending one number.
///
/// # Errors
///
/// [`Refused`], from [`ActionLog::set`], for a row the log will not take.
pub fn absorb<A: Clone + Default + PartialEq>(
    log: &mut ActionLog<A>,
    frontier: &mut Frontier,
    datagram: &Datagram<A>,
) -> Result<Correction, Refused> {
    let seat = datagram.seat;
    let mut answer = Correction::Duplicate;
    let mut rows = datagram.ticks().peekable();
    while let Some((at, action)) = rows.next() {
        // The same tick named again later in the same datagram carries the row
        // this one was padding for.
        if rows.peek().is_some_and(|(next, _)| *next == at) {
            continue;
        }
        // A row the log has already let go of. Nothing here can say anything
        // about a tick whose actions are gone.
        if at < log.first() {
            continue;
        }
        // And one it has not made room for yet, which is the sender being ahead
        // of this machine rather than anything being wrong.
        if at >= log.last() {
            continue;
        }

        if log.is_confirmed(at, seat) {
            if log.get(at, seat) == Some(action) {
                continue;
            }
            return Ok(Correction::Contradiction { at });
        }

        // A seat with nothing to repeat was predicted `A::default()`, so that
        // is what the arriving action has to match for the prediction to have
        // held.
        let agrees = action_at(log, frontier, at, seat)
            .map_or_else(|| *action == A::default(), |had| had == action);
        log.set(at, seat, action.clone())?;
        answer = match (answer, agrees) {
            (Correction::Mispredicted { at: had }, _) => Correction::Mispredicted { at: had },
            (_, true) => Correction::Agreed,
            (_, false) => Correction::Mispredicted { at },
        };
    }
    confirm_contiguously(log, frontier, seat);
    Ok(answer)
}

/// Moves a seat's frontier up to the newest row it has **with no gap below
/// it**.
///
/// This is the difference between "the newest thing I have heard from you" and
/// "everything you have said up to here", and only the second one is safe to
/// call confirmed. A datagram carries a window of rows, so a peer that loses
/// more of them in a row than the window is wide receives a datagram whose head
/// is well past a hole -- and a frontier that jumped to the head would report
/// ticks as agreed while the rows under them were still this machine's
/// guesses. Two peers guessing differently about the same hole is a divergence
/// that no retransmission can fix afterwards, because both have already
/// declared those ticks final.
///
/// With this, a hole simply stops the frontier: the peer predicts, waits, and
/// carries on the moment the missing rows arrive -- which they do, because a
/// datagram's window reaches back to what the far end has acknowledged.
fn confirm_contiguously<A>(log: &ActionLog<A>, frontier: &mut Frontier, seat: PlayerId) {
    let first = log.first();
    let mut probe = frontier.confirmed(seat).map_or(first, Tick::next);
    if probe < first {
        // Rows a bounded session has already let go of are behind this peer
        // rather than missing from it.
        probe = first;
    }
    let mut newest = None;
    while probe < log.last() && log.is_confirmed(probe, seat) {
        newest = Some(probe);
        probe = probe.next();
    }
    if let Some(newest) = newest {
        frontier.observe(seat, newest);
    }
}
