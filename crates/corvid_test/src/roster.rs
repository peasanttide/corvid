//! Rebuilding the slice of players a tick sees, from the session and nothing
//! else.

use corvid_behavior::{PlayerId, PlayerState, State};
use corvid_replay::{ActionLog, Opening};
use corvid_time::Tick;

/// Fills `into` with the roster tick `at` was simulated against.
///
/// The same three sources
/// [`Session::seek`](corvid_replay::Session::seek) reads, in the same order and
/// with the same fallbacks: the seat is the roster's position, the presence is
/// [`Profile::presence_at`](corvid_replay::Profile::presence_at), and the action
/// is the log's or the idle one. A check that rebuilt the roster any other way
/// would be checking a session nobody played.
pub(crate) fn seat<'a, S: State>(
    opening: &'a Opening<S>,
    log: &'a ActionLog<S::Action>,
    at: Tick,
    idle: &'a S::Action,
    into: &mut Vec<PlayerState<S::Action>>,
) {
    into.clear();
    for (index, profile) in opening.roster.iter().enumerate() {
        // A roster longer than a `PlayerId` can address stops here rather than
        // folding the seats past the end onto the last addressable one, which is
        // what `seek` does with the same roster and for the same reason.
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        let id = PlayerId(index);
        let Some(presence) = profile.presence_at(at) else {
            continue;
        };
        into.push(PlayerState {
            id,
            presence,
            action: log.get(at, id).cloned().unwrap_or_else(|| idle.clone()),
        });
    }
}
