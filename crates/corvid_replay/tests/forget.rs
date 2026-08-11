//! Forgetting a prefix of an action log.
//!
//! What a log holds afterwards: that every retained entry reads exactly as it
//! did, that a forgotten prefix is indistinguishable from one never held, and
//! that a correction carried by a retained row is still carried.
//!
//! `tests/forget_session.rs` holds the same operation on a whole session.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, scripted};
use corvid_behavior::PlayerId;
use corvid_replay::ActionLog;
use corvid_time::Tick;
/// The tick every test here forgets to.
///
/// Thirteen rows of four seats is fifty-two entries, which is not a whole number
/// of bytes. A confirmation bitmap that was drained a byte at a time rather than
/// shifted a bit at a time passes at every multiple of eight and fails here,
/// which is why the boundary is this one rather than a round number.
const HORIZON: Tick = Tick(13);

/// Whether seat `player` has confirmed its action for `tick`, in the mixed
/// pattern the bitmap tests are written against.
///
/// A log where everything is confirmed cannot tell a bitmap that was shifted
/// correctly from one that was shifted by rows instead of by entries, because
/// every bit it could land on holds the same value. This leaves about a third of
/// the entries unconfirmed and does it without a period of eight, so a shift
/// that is out by any number of bits moves a boundary somebody can see.
fn speaks(tick: Tick, player: PlayerId) -> bool {
    let mixed = tick
        .0
        .wrapping_mul(0xd6e8_feb8_6659_fd93)
        .wrapping_add(u64::from(player.0).wrapping_mul(0x9e37_79b9));
    (mixed >> 41) % 3 != 0
}

/// A log of `ticks` rows from `first`, with [`scripted`] actions in the entries
/// [`speaks`] confirms and nothing in the rest.
fn mixed(first: Tick, ticks: u64, players: u16) -> ActionLog<Action> {
    let mut log = ActionLog::new(first, players);
    if ticks == 0 {
        return log;
    }
    log.extend_to(Tick(first.0 + ticks - 1))
        .expect("the log grows from its own first tick");
    for row in 0..ticks {
        let tick = Tick(first.0 + row);
        for seat in 0..players {
            let player = PlayerId(seat);
            if speaks(tick, player) {
                log.set(tick, player, scripted(tick, player))
                    .expect("a fresh log has nothing confirmed to contradict");
            }
        }
    }
    log
}

#[test]
fn every_retained_entry_reads_exactly_as_it_did() {
    let mut log = mixed(Tick::ZERO, 40, 4);
    let before: Vec<(Tick, PlayerId, Action, bool)> = (0..40)
        .flat_map(|row| {
            (0..4).map(move |seat| {
                let (tick, player) = (Tick(row), PlayerId(seat));
                (tick, player, scripted(tick, player), speaks(tick, player))
            })
        })
        .collect();

    log.forget_before(HORIZON);

    assert_eq!(log.first(), HORIZON);
    assert_eq!(log.last(), Tick(40), "the frontier does not move");
    assert_eq!(log.ticks(), 27);

    for (tick, player, action, confirmed) in before {
        if tick < HORIZON {
            assert_eq!(log.get(tick, player), None, "{tick} is before the horizon");
            continue;
        }
        // An unconfirmed entry holds the default, which is what it held before
        // the forget and is not what `scripted` would have put there.
        let expected = if confirmed { action } else { Action::default() };
        assert_eq!(log.get(tick, player), Some(&expected), "at {tick}");
        assert_eq!(
            log.is_confirmed(tick, player),
            confirmed,
            "the confirmation bit for seat {} at {tick}",
            player.0,
        );
    }
}

#[test]
fn a_log_that_forgot_a_prefix_is_a_log_that_never_held_it() {
    let mut forgotten = mixed(Tick::ZERO, 40, 4);
    forgotten.forget_before(HORIZON);

    // Built from the same script over the rows that are left, and never
    // extended past them. This compares the parts `get` and `is_confirmed`
    // cannot reach: the bitmap byte for byte, which is where a shift that moved
    // by rows rather than by entries shows up.
    let fresh = mixed(HORIZON, 27, 4);

    assert_eq!(forgotten, fresh);
    assert_eq!(forgotten.entries(), fresh.entries());
    assert_eq!(forgotten.confirmed_bytes(), fresh.confirmed_bytes());
}

/// The four fields an [`ActionLog`] is written down as, so that a test can
/// write one down that nothing in this crate would ever produce.
///
/// The names, the order and the types are the log's own, which is what makes
/// this decode as one: `corvid_wire` is `bincode`, so a struct is its fields in
/// order and the generation the log skips is not among them.
#[derive(serde::Serialize)]
struct HandMade {
    /// The tick the first row belongs to.
    first: Tick,
    /// How many seats wide a row is.
    players: u16,
    /// The entries, row-major.
    actions: Vec<Action>,
    /// The confirmation bitmap, however many bits of it are set.
    confirmed: Vec<u8>,
}

#[test]
fn a_capture_with_bits_set_past_its_entries_is_cleaned_by_a_forget() {
    // The one case the high-bit mask at the end of `forget_confirmations`
    // exists for, and it is not the shift: the shift brings zeros down into the
    // bits it vacates, so a log built by this crate's own constructors is
    // already clean above its last entry. A *decoded* log is not. `Deserialize`
    // takes the bitmap verbatim and `Session::check` compares its length
    // against the entries rather than its contents, so a corrupt or hand-made
    // capture can carry bits past its last entry through every check there is.
    //
    // Fifteen entries is one bit short of two whole bytes, so bit fifteen is
    // exactly such a bit -- and forgetting one row of three shifts by three,
    // which lands it inside the last byte of what is left rather than past the
    // end of the bitmap, where the truncation would have taken it anyway.
    const SEATS: u16 = 3;
    const ROWS: usize = 5;
    let capture = |confirmed: Vec<u8>| HandMade {
        first: Tick::ZERO,
        players: SEATS,
        actions: vec![Action::Bump; ROWS * usize::from(SEATS)],
        confirmed,
    };
    let load = |made: &HandMade| -> ActionLog<Action> {
        corvid_wire::decode(&corvid_wire::encode(made).expect("a hand-made capture encodes"))
            .expect("and decodes as the log it is shaped like")
    };

    let mut clean = load(&capture(vec![0b1010_1101, 0b0010_1010]));
    let mut dirty = load(&capture(vec![0b1010_1101, 0b1010_1010]));

    // The two really do differ, and only above the last entry: every bit a
    // reader can reach agrees, and the bitmaps do not.
    assert_ne!(
        clean, dirty,
        "the doctored capture is the same as the clean one"
    );
    for row in 0..ROWS {
        for seat in 0..SEATS {
            let (tick, seat) = (Tick(row as u64), PlayerId(seat));
            assert_eq!(
                clean.is_confirmed(tick, seat),
                dirty.is_confirmed(tick, seat),
                "the confirmation for seat {} at {tick} was doctored too",
                seat.0,
            );
        }
    }

    clean.forget_before(Tick(1));
    dirty.forget_before(Tick(1));

    assert_eq!(
        clean, dirty,
        "a forget left a bit set past the last entry, so a log that came off a \
         disk is unequal to the same session played",
    );
}

#[test]
fn a_correction_a_retained_row_carries_is_still_carried() {
    let mut log = mixed(Tick::ZERO, 40, 4);
    // A row inside the stretch that survives, and one inside the stretch that
    // does not. Only the first is a correction a retained state depends on.
    log.set(Tick(20), PlayerId(0), Action::Reset)
        .expect("seat zero said nothing at tick 20");
    log.set(Tick(3), PlayerId(1), Action::Reset)
        .expect("seat one said nothing at tick 3");

    let before: Vec<(Tick, u64)> = (14..=41)
        .map(|row| (Tick(row), log.generation_at(Tick(row))))
        .collect();
    let counts: Vec<u64> = before.iter().map(|(_, count)| *count).collect();
    assert!(
        counts.first() != counts.last(),
        "the fixture has to take a correction inside the retained stretch, or \
         this test compares a column of one number against itself"
    );

    log.forget_before(HORIZON);

    for (tick, generation) in before {
        assert_eq!(log.generation_at(tick), generation, "at {tick}");
    }

    // The horizon itself is the one tick whose count moves, and it moves to
    // zero: the rows its state was built from are exactly the ones that have
    // gone, so this log no longer knows of a correction that touched them.
    // `ActionLog::forget_before` says which direction that fails a snapshot
    // ring in.
    assert_eq!(log.generation_at(HORIZON), 0);
}
