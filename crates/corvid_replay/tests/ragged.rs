//! Captures that are the wrong shape inside, and what a log does with them.
//!
//! Distinct from the refusals in `tests/roundtrip.rs`: everything here decodes,
//! and the question is what the log then holds. A bitmap with spare bits, a row
//! that stops partway, entries with no row to belong to -- each has an answer
//! that is neither a refusal nor a silent acceptance, and each of those answers
//! is a decision rather than a consequence.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Counter, opening, play, schema};
use corvid_behavior::PlayerId;

use corvid_hash::digest;
use corvid_replay::{ActionLog, HashTrace, Load, Opening, Refused, Session, Shape, Snapshots};
use corvid_time::Tick;
use serde::Serialize;

/// Room for every state a two-hundred-tick session produces.
const ROOMY: usize = 1 << 24;

/// An action log's four recorded fields, declared here so that a capture can be
/// written down with a confirmation bitmap the real type would never build.
///
/// The compact encoding writes fields in declaration order with no names, so
/// this is byte-for-byte an `ActionLog<Action>` as far as a decoder is
/// concerned -- which is exactly the position a corrupt file or a hand-made
/// capture puts `load` in.
#[derive(Serialize)]
struct MirroredLog<'a> {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// The entries, row-major.
    actions: &'a [Action],
    /// The bitmap, at whatever length this test wants.
    confirmed: Vec<u8>,
}

/// A session with one of those in place of its log.
#[derive(Serialize)]
struct MirroredSession<'a> {
    /// The real opening.
    opening: &'a Opening<Counter>,
    /// The mirrored log.
    log: MirroredLog<'a>,
    /// The real trace.
    marks: &'a HashTrace,
}

/// The bytes of a capture whose log holds `entries` actions and a bitmap of
/// `confirmed_bytes` bytes, at the session's own first tick and width.
fn capture_with_log(session: &Session<Counter>, entries: usize, confirmed_bytes: usize) -> Vec<u8> {
    let actions: Vec<Action> = (0..entries).map(|_| Action::Idle).collect();
    corvid_wire::encode(&MirroredSession {
        opening: &session.opening,
        log: MirroredLog {
            first: session.log.first(),
            players: session.log.players(),
            actions: &actions,
            confirmed: vec![0xff; confirmed_bytes],
        },
        marks: &session.marks,
    })
    .unwrap()
}

#[test]
fn a_capture_whose_confirmations_do_not_cover_its_actions_is_refused() {
    // The fifth way a capture disagrees with itself, and the one that is not
    // about indexing. A bit past the end of the bitmap reads as zero, so every
    // entry it does not cover is *unconfirmed* -- and an unconfirmed entry can be
    // written to. A capture a byte short would let a peer rewrite, one at a
    // time, actions the session has already agreed on and simulated, and
    // `Refused::Confirmed` would never fire.
    let session = play(20);
    let entries = session.log.entries();
    let needed = entries.div_ceil(8);
    assert!(needed > 1, "the fixture has to need more than one byte");

    for bytes in [0, needed - 1, needed + 1] {
        assert_eq!(
            Session::<Counter>::load(&capture_with_log(&session, entries, bytes), schema())
                .unwrap_err(),
            Load::Shape(Shape::Confirmations {
                bytes,
                needed,
                entries,
            }),
            "a bitmap of {bytes} bytes loaded",
        );
    }

    // The mirror is a faithful one, so the refusals above are about the bitmap
    // and not about the mirror being a different type: at the right length the
    // same bytes load.
    assert!(
        Session::<Counter>::load(&capture_with_log(&session, entries, needed), schema()).is_ok()
    );
}

#[test]
fn a_capture_whose_entries_stop_partway_through_a_row_is_refused() {
    // The sixth way, and the one that reads as harmless. Ten entries in rows of
    // four is two whole rows and two entries over; the capture disagrees with
    // none of the other five checks, and the log inside it answers
    // `ticks() == 2` like any other two-row log. The test below builds that log
    // and runs the session on one more tick, which is where the difference is.
    let session = play(2);
    assert_eq!(session.log.players(), 4);
    let ragged = capture_with_log(&session, 10, 10_usize.div_ceil(8));
    assert_eq!(
        Session::<Counter>::load(&ragged, schema()).unwrap_err(),
        Load::Shape(Shape::Ragged {
            entries: 10,
            players: 4,
        }),
    );

    // Eight entries is two whole rows of the same width, so the refusal is
    // about the remainder rather than about the number ten.
    assert!(Session::<Counter>::load(&capture_with_log(&session, 8, 1), schema()).is_ok());
}

#[test]
fn a_ragged_log_hands_its_orphan_entries_to_the_next_row_it_grows() {
    // What the refusal above is worth, shown rather than asserted about. The
    // two entries past the last whole row are unreachable exactly as long as
    // the log stays this length -- which for a live session is until the next
    // tick.
    let mut log: ActionLog<Action> = corvid_wire::decode(
        &corvid_wire::encode(&MirroredLog {
            first: Tick::ZERO,
            players: 4,
            actions: &[Action::Bump; 10],
            confirmed: vec![0xff; 2],
        })
        .unwrap(),
    )
    .unwrap();

    assert_eq!(log.ticks(), 2);
    assert_eq!(log.get(Tick(2), PlayerId(0)), None);

    // One more tick, and row 2 arrives already half full of actions this
    // capture never recorded a row for. The two seats that inherited an entry
    // keep its confirmation; the two that did not are new and are unconfirmed,
    // which is `extend_to`'s doing and the next test's subject.
    log.extend_to(Tick(2)).unwrap();
    assert_eq!(
        log.row(Tick(2)),
        [Action::Bump, Action::Bump, Action::Idle, Action::Idle],
    );
    assert!(log.is_confirmed(Tick(2), PlayerId(0)));
    assert!(!log.is_confirmed(Tick(2), PlayerId(2)));

    // So seat 0's real action for tick 2 is refused as a contradiction of one
    // nobody sent, which is the log's authority pointed the wrong way.
    assert_eq!(
        log.set(Tick(2), PlayerId(0), Action::Reset),
        Err(Refused::Confirmed {
            tick: Tick(2),
            player: PlayerId(0),
        }),
    );
}

#[test]
fn a_row_that_appears_is_unconfirmed_whatever_the_bitmap_had_spare() {
    // The neighbour of the check above, which no check can see. Twelve entries
    // in rows of four is three whole rows and needs two bytes, so four bits of
    // the second byte belong to no entry -- and a capture may set them, because
    // its length is all there is to compare. Those four bits are the ones the
    // next row lands on.
    let capture = MirroredLog {
        first: Tick::ZERO,
        players: 4,
        actions: &[Action::Bump; 12],
        confirmed: vec![0xff; 2],
    };
    let mut log: ActionLog<Action> =
        corvid_wire::decode(&corvid_wire::encode(&capture).unwrap()).unwrap();
    assert_eq!(log.ticks(), 3);

    log.extend_to(Tick(3)).unwrap();
    for seat in 0..4 {
        assert!(
            !log.is_confirmed(Tick(3), PlayerId(seat)),
            "seat {seat} of a row that had just appeared was already confirmed",
        );
    }

    // Which is what makes the row usable: every seat's first packet is taken,
    // and the confirmations the capture did record are still there.
    log.set(Tick(3), PlayerId(0), Action::Reset).unwrap();
    assert_eq!(log.get(Tick(3), PlayerId(0)), Some(&Action::Reset));
    assert!(log.is_confirmed(Tick(2), PlayerId(3)));
}

#[test]
fn an_opening_with_more_seats_than_a_player_id_can_address_has_no_session() {
    // The constructor's one refusal. A log is as wide as `Opening::seats`, so a
    // roster of seventy thousand has no width that is not a lie: saturating
    // would build a log 65 535 wide against a roster naming 70 000, which is
    // the disagreement `check` refuses -- handed back by the call whose job is
    // to make sessions that agree with themselves.
    let mut wide = opening();
    wide.roster = vec![wide.roster[0]; 70_000];
    assert_eq!(wide.seats(), None);
    assert_eq!(
        Session::<Counter>::new(wide).err(),
        Some(Shape::Roster { seats: 70_000 }),
    );

    // And exactly `u16::MAX` seats is a session, so the refusal is about the
    // boundary rather than about a large roster.
    let mut widest = opening();
    widest.roster = vec![widest.roster[0]; usize::from(u16::MAX)];
    let session = Session::<Counter>::new(widest).expect("65 535 seats each have a column");
    assert_eq!(session.log.players(), u16::MAX);
    assert_eq!(session.check(), Ok(()));
}

#[test]
fn a_short_bitmap_costs_the_log_its_rows_rather_than_its_authority() {
    // Why the check above is worth its place, shown rather than asserted about
    // -- and what the log does anyway for a caller who never asked it.
    //
    // An entry whose confirmation bit has nowhere to go is refused rather than
    // written. The alternative is the one thing a log may not do: accept the
    // write, drop the bit, and so accept every
    // contradicting write after it as well, because `Refused::Confirmed` can
    // only fire against a bit that was recorded. `Session::check` is still the
    // call that names the malformed capture, but `ActionLog` is public and
    // `corvid_wire::decode` reaches it without passing through `Session::load`,
    // so the refusal has to be here too.
    let session = play(20);
    let mut log: ActionLog<Action> = corvid_wire::decode(
        &corvid_wire::encode(&MirroredLog {
            first: session.log.first(),
            players: session.log.players(),
            actions: &(0..session.log.entries())
                .map(|_| Action::Bump)
                .collect::<Vec<_>>(),
            confirmed: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();

    assert_eq!(log.get(Tick(4), PlayerId(1)), Some(&Action::Bump));
    assert!(!log.is_confirmed(Tick(4), PlayerId(1)));
    assert_eq!(
        log.set(Tick(4), PlayerId(1), Action::Reset),
        Err(Refused::Beyond {
            tick: Tick(4),
            first: log.first(),
            rows: log.ticks(),
        }),
        "a row the bitmap does not cover is not a row that can be written",
    );
    // And the refusal left the entry alone rather than writing it and losing
    // the bit, which is the whole difference.
    assert_eq!(log.get(Tick(4), PlayerId(1)), Some(&Action::Bump));
    assert!(!log.is_confirmed(Tick(4), PlayerId(1)));

    // A log whose bitmap does cover its entries refuses the same write, by the
    // name that says why: the row is there and is already spoken for.
    let mut honest = play(20).log;
    let was = *honest.get(Tick(4), PlayerId(1)).unwrap();
    let other = if was == Action::Reset {
        Action::Bump
    } else {
        Action::Reset
    };
    assert!(honest.set(Tick(4), PlayerId(1), other).is_err());
}

#[test]
fn a_session_assembled_by_hand_can_be_checked_with_the_same_call() {
    // `new` cannot build an inconsistent session -- it builds the log and the
    // trace from the opening -- so the disagreement always arrives afterwards,
    // through the public fields, where no constructor can see it. What is on
    // offer instead is the check itself.
    let mut session = play(20);
    assert_eq!(session.check(), Ok(()));

    session.log = ActionLog::new(session.opening.first, 3);
    assert_eq!(session.check(), Err(Shape::Width { log: 3, roster: 4 }),);

    // And it is the same call `load` makes, so a capture and a hand-assembled
    // session are refused for the same reason with the same value.
    let bytes = session.save().unwrap();
    assert_eq!(
        Session::<Counter>::load(&bytes, schema()).unwrap_err(),
        Load::Shape(session.check().unwrap_err()),
    );
}

#[test]
fn a_loaded_session_still_keys_its_snapshots_to_its_log() {
    // The generation is not written down, so a decoded log has to be given one
    // -- a row apiece, at zero. A log that came back with none would report
    // generation zero for every tick forever, count no correction a rollback
    // then made, and hand a seek the snapshot the correction invalidated. That
    // is the whole hazard of `tests/seek.rs` reappearing on the far side of a
    // save, which is exactly where nobody would look for it.
    let mut session = Session::<Counter>::load(&play(20).save().unwrap(), schema()).unwrap();

    // Seat 1 at tick 15 is the entry this session never confirmed, so writing it
    // is a correction rather than a first confirmation.
    let hole = (Tick(15), PlayerId(1));
    session.log = {
        let mut log = ActionLog::new(session.opening.first, common::seats(&session));
        log.extend_to(Tick(19)).unwrap();
        for tick in 0..20 {
            for seat in 0..common::seats(&session) {
                if (Tick(tick), PlayerId(seat)) != hole {
                    log.set(Tick(tick), PlayerId(seat), Action::Bump).unwrap();
                }
            }
        }
        corvid_wire::decode(&corvid_wire::encode(&log).unwrap()).unwrap()
    };
    assert_eq!(session.log.generation(), 0, "a decoded log starts at zero");

    let mut snapshots = Snapshots::new(ROOMY);
    let (before, _) = session.seek(&mut snapshots, Tick(20)).unwrap();
    let predicted = digest(&before);
    drop(before);
    assert!(snapshots.ticks().any(|tick| tick == Tick(20)));

    session.log.set(hole.0, hole.1, Action::Reset).unwrap();
    assert_eq!(
        session.log.generation(),
        1,
        "the correction was not counted"
    );

    let (after, _) = session.seek(&mut snapshots, Tick(20)).unwrap();
    assert_ne!(
        digest(&after),
        predicted,
        "the seek handed back the snapshot the correction invalidated",
    );
}
