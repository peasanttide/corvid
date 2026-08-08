//! The claim "a captured run replays to the same state", as one call.

use corvid_app::Outcome;
use corvid_behavior::{Discard, Player, State};
use corvid_hash::digest;
use corvid_replay::{HashTrace, LevelRef, Session};

use crate::{Diverged, Failed, What, roster::seat};

/// Writes a run down, reads it back, replays it from its opening, and compares
/// every tick against the marks the run recorded live.
///
/// This is the claim a capture exists to support. A session is the opening plus
/// one action per seat per tick; every state is a pure function of those, so a
/// capture that cannot be replayed into the states it recorded is a capture of
/// nothing.
///
/// Four comparisons, in this order, because each one makes the next meaningful:
///
/// | | What it catches |
/// |---|---|
/// | how far the decoded log reaches | a log that lost rows on the way down, which would make every later comparison run over a shorter session and pass |
/// | the decoded trace against the live one | a column of digests that did not survive being written down |
/// | each replayed state's digest against the live mark | the replay itself: a level, a set of rules or an origin that came back subtly different |
/// | the final state by [`Eq`] | the field a game's `Hash` does not absorb and its `Eq` does |
///
/// The first two are guards on the encoding rather than on the game, and today
/// nothing can make either of them fire: `corvid_wire` writes a log's rows and a
/// trace's marks back as it read them, and
/// [`Session::load`](corvid_replay::Session::load) refuses — as
/// [`Failed::Read`] — every shape whose row count would come back different.
/// They stay because the day one of those two sentences stops being true is the
/// day every comparison after them starts running over the wrong session and
/// passing, which is the failure a round-trip check exists to prevent. What
/// covers them is `survived_the_round_trip`, a private helper this file's own
/// tests hand the two decoded sessions this function cannot construct.
///
/// # What the round trip does and does not check
///
/// The bytes go through `corvid_wire`, which is the encoding a capture is
/// written in, so a `State`, `Level`, `Rules` or `Action` that cannot be written
/// down compactly fails here rather than on somebody's disk.
///
/// The schema is **not** checked. [`Session::load`](corvid_replay::Session::load)
/// compares the recorded schema digest against the one it is handed, and the one
/// handed here is the capture's own, so that comparison compares a number with
/// itself and can only pass. Checking it would mean knowing this build's schema,
/// which is the game's to state and not this crate's to guess. A game that wants
/// the schema checked calls `Session::load` with its own.
///
/// Nothing here reads a directory. What is round-tripped is
/// [`Outcome::session`], not the files
/// [`App::capture`](corvid_app::App::capture) wrote — an [`Outcome`] does not
/// record where it was captured to, and the bytes are the same bytes either way,
/// since `capture` writes exactly [`Session::save`](corvid_replay::Session::save).
/// What compares a directory is
/// [`matches_goldens`](crate::matches_goldens).
///
/// # Errors
///
/// [`Failed::Wrote`] if the session could not be encoded, [`Failed::Read`] if
/// the bytes did not read back as a session this build can replay, and
/// [`Failed::Diverged`] naming the first tick the replay and the run disagree
/// about.
pub fn replays_to_itself<S: State>(outcome: &Outcome<S>) -> Result<(), Failed<LevelRef<S>>> {
    let live = &outcome.session;
    let bytes = live.save().map_err(Failed::Wrote)?;
    let session: Session<S> = Session::load(&bytes, live.opening.schema).map_err(Failed::Read)?;

    let first = session.first();
    if let Some(diverged) = survived_the_round_trip(live, &session) {
        return Err(diverged.into());
    }

    // The replay. The recorded side is the live trace rather than the decoded
    // one — they have just been compared, and using the trace that was never
    // written down is what keeps this a comparison between what ran and what
    // came back rather than between the capture and itself.
    let opening = &session.opening;
    // A value rather than the handle the opening holds. Nothing here shares a
    // state — the walk owns each one until the next replaces it — so the one
    // deep clone at the top is the whole cost, where carrying an `Arc` would
    // allocate once per tick to hold something no second reader ever sees.
    let mut state = S::clone(&opening.origin());
    let idle = S::Action::default();
    let mut roster: Vec<Player<'_, S::Action>> = Vec::new();
    let mut at = first;

    loop {
        // The opening mark is not a state's digest — it covers the level as
        // well, so that a peer on a different build of the same file disagrees
        // here rather than once the contents start mattering. `Opening::mark`
        // is the one definition of it, and this walk has to use the same one
        // the live session opened with or every capture reports as diverged at
        // its first tick.
        let computed = if at == first {
            opening.mark()
        } else {
            digest(&state)
        };
        if live.marks.get(at) != Some(computed) {
            return Err(Diverged::walked(
                first,
                at,
                What::Marks {
                    recorded: live.marks.get(at).unwrap_or(computed),
                    computed,
                },
            )
            .into());
        }
        if at >= session.last() {
            break;
        }

        seat(opening, &session.log, at, &idle, &mut roster);
        // A `Discard`: this walk is re-simulating ticks that already ran, and
        // a request re-issued by a replay would save a file for a second time.
        let next = S::clone(&state).tick(
            &opening.content,
            &roster,
            &opening.rules,
            &mut Discard::new(),
        );
        state = next;
        at = at.next();
    }

    // Every digest agreed, so anything left is a difference the digest cannot
    // see. `Data` demands `Eq` as well as `Hash` precisely because the two
    // can disagree, and a rollback decides whether its prediction held with the
    // first while a desync check decides whether two peers agree with the
    // second.
    if state != *outcome.state {
        return Err(Diverged {
            agreed_through: (at > first).then(|| at.prev()),
            at,
            what: What::Unequal {
                digest: digest(&state),
            },
        }
        .into());
    }
    Ok(())
}

/// Whether what came back off the wire describes the same session as what went
/// down: the same reach, and the same column of marks.
///
/// The reach first, because it bounds the other comparison. A decoded log that
/// lost rows would make the trace comparison — and the replay after it — run
/// over the shorter of the two and agree, so a check that compared the marks
/// first would report a session that survived when half of it had not.
///
/// A function of its own rather than four lines inside
/// [`replays_to_itself`] because it is the one part of that check its own caller
/// cannot reach: `replays_to_itself` decodes what it just encoded, and every
/// decoded session it can produce is equal to the live one. Handing it the two
/// sessions is what lets a test say what it does with a pair that differs.
fn survived_the_round_trip<S: State>(
    live: &Session<S>,
    decoded: &Session<S>,
) -> Option<Diverged<LevelRef<S>>> {
    if decoded.last() != live.last() {
        let ends = decoded.last().min(live.last());
        return Some(Diverged {
            agreed_through: Some(ends),
            at: ends.next(),
            what: What::Reach {
                recorded: live.last(),
                computed: decoded.last(),
            },
        });
    }

    let at = live.marks.disagrees_with(&decoded.marks)?;
    Some(Diverged::walked(
        live.first(),
        at,
        marks(&live.marks, &decoded.marks, at),
    ))
}

/// The two marks at `at`, as the difference between them.
///
/// A tick both traces have a mark for is the only thing
/// [`HashTrace::disagrees_with`](corvid_replay::HashTrace::disagrees_with)
/// reports, so both are present; the fallback exists because
/// [`HashTrace::get`](corvid_replay::HashTrace::get) answers an [`Option`] and
/// this crate does not unwrap.
fn marks<R>(recorded: &HashTrace, computed: &HashTrace, at: corvid_time::Tick) -> What<R> {
    What::Marks {
        recorded: recorded.get(at).unwrap_or(corvid_hash::Digest::ZERO),
        computed: computed.get(at).unwrap_or(corvid_hash::Digest::ZERO),
    }
}

#[cfg(test)]
mod tests {
    //! What [`survived_the_round_trip`] does with a pair of sessions that
    //! differ, which is the one thing [`replays_to_itself`] cannot show it: that
    //! function decodes what it just encoded, so every decoded session it can
    //! produce is equal to the live one and neither comparison can fire. The
    //! pairs are built here instead.

    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
    )]

    use std::sync::Arc;

    use corvid_behavior::{Command, PlayerId, State};

    use corvid_hash::Digest;
    use corvid_hash::digest;
    use corvid_replay::{Opening, Profile, Schema, Seed, Session};
    use corvid_time::Tick;

    use super::{Diverged, What, survived_the_round_trip};

    /// A level with nothing in it.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    struct Nowhere;

    impl corvid_behavior::Level for Nowhere {
        type Reference = String;
        fn load(
            _reference: &String,
            _files: &dyn corvid_behavior::Source,
        ) -> Result<Self, corvid_behavior::Malformed> {
            Ok(Self)
        }
    }

    /// A game that counts, which is as much as a session of a fixed shape needs.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    struct Steps(i64);

    impl State for Steps {
        const NAME: &'static str = "steps";

        type Level = Nowhere;
        type Rules = ();
        type Action = ();

        fn tick(
            self,
            _level: &Nowhere,
            _players: &[corvid_behavior::Player<'_, ()>],
            _rules: &(),
            _command: &mut impl Command<Reference = String>,
        ) -> Self {
            Self(self.0 + 1)
        }
    }

    /// A session of `rows` ticks, marked with what [`Steps`] would have
    /// computed.
    fn played(rows: u64) -> Session<Steps> {
        let opening = Opening::<Steps> {
            level: "nowhere".to_owned(),
            content: Arc::new(Nowhere),
            rules: Arc::new(()),
            roster: vec![Profile {
                account: corvid_behavior::ProfileId(1),
                joined: Tick::ZERO,
                left: None,
            }],
            seed: Seed(1),
            first: Tick::ZERO,
            origin: None,
            schema: Schema::new("steps").field("State", "i64").digest(),
        };
        let mut session = Session::new(opening).unwrap();
        for row in 0..rows {
            let at = Tick(row);
            session.log.extend_to(at).unwrap();
            session.log.set(at, PlayerId(0), ()).unwrap();
            session.marks.push(digest(&i64::try_from(row + 1).unwrap()));
        }
        session.check().unwrap();
        session
    }

    #[test]
    fn two_sessions_that_are_the_same_session_survived_it() {
        // The one that has to pass. Without it both tests below would be
        // satisfied by a comparison that reported a difference unconditionally.
        assert!(survived_the_round_trip(&played(6), &played(6)).is_none());
    }

    #[test]
    fn a_decoded_log_that_lost_rows_is_a_reach_and_not_a_mark() {
        // A log that came back short would make every comparison after it run
        // over the shorter of the two sessions and agree, which is the failure
        // this guard exists for: the four marks the two do share are identical,
        // so a check that compared the marks first would report that the
        // capture survived.
        let (live, decoded) = (played(6), played(4));
        let diverged =
            survived_the_round_trip(&live, &decoded).expect("the two are not one session");

        assert_eq!(
            diverged,
            Diverged {
                agreed_through: Some(Tick(4)),
                at: Tick(5),
                what: What::Reach {
                    recorded: Tick(6),
                    computed: Tick(4),
                },
            },
        );
        // And the message says which side reached where, rather than that
        // something differs.
        let message = diverged.to_string();
        assert!(message.contains("reached tick 6"), "{message}");
        assert!(message.contains("reached tick 4"), "{message}");
    }

    #[test]
    fn a_mark_that_did_not_survive_is_named_at_the_tick_it_sits_at() {
        // The column of digests, compared before a single tick is replayed. A
        // trace that lost a mark on the way down would otherwise be found by the
        // replay instead — at the same tick, but reported as the game computing
        // something different, which sends a reader looking at the arithmetic
        // rather than at the encoding.
        let live = played(6);
        let mut decoded = played(6);
        // Rewritten from tick three on, since a trace is a column that is
        // pushed rather than indexed into. The marks after the third are the
        // ones the live session has, so the two disagree at exactly one tick.
        decoded.marks.truncate_from(Tick(3));
        decoded.marks.push(Digest::ZERO);
        for row in 4..=6 {
            decoded
                .marks
                .push(live.marks.get(Tick(row)).expect("a mark the live run made"));
        }

        let diverged =
            survived_the_round_trip(&live, &decoded).expect("the two are not one session");
        assert_eq!(diverged.at, Tick(3));
        assert_eq!(diverged.agreed_through, Some(Tick(2)));
        let What::Marks { recorded, computed } = diverged.what else {
            panic!("{diverged}");
        };
        assert_eq!(recorded, live.marks.get(Tick(3)).unwrap());
        assert_eq!(computed, Digest::ZERO);
    }
}
