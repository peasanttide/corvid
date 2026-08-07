//! The ring itself: what it charges, what it keeps, what it throws away, and
//! what a budget spent is a budget given back.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Counter, State, opening, play, quiet_log};
use corvid_replay::Snapshots;
use corvid_time::Tick;
/// A state whose encoded length is a known, comfortable size.
const fn state(count: i64) -> State {
    State {
        count,
        folded: 0,
        movers: Vec::new(),
        roster: Vec::new(),
    }
}

/// What one of those costs the ring, measured the way the ring measures it.
fn charged_for(one: &State) -> usize {
    let mut ring: Snapshots<Counter> = Snapshots::new(usize::MAX);
    assert!(ring.keep(&quiet_log(), Tick::ZERO, one));
    ring.charged()
}

#[test]
fn a_state_that_does_not_fit_alone_is_not_kept() {
    let mut ring: Snapshots<Counter> = Snapshots::new(1);
    assert!(!ring.keep(&quiet_log(), Tick::ZERO, &state(1)));
    assert!(ring.is_empty());
    assert_eq!(ring.charged(), 0);
    assert_eq!(ring.nearest(&quiet_log(), Tick::ZERO), None);
}

#[test]
fn keeping_the_same_tick_twice_replaces_rather_than_accumulates() {
    // A rollback reaches the same tick again with a state built from a
    // corrected log. A ring that appended would hold two states for one tick
    // and charge for both, and `nearest` would have to pick between them.
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 8);

    assert!(ring.keep(&quiet_log(), Tick(5), &state(1)));
    assert!(ring.keep(&quiet_log(), Tick(5), &state(2)));
    assert_eq!(ring.len(), 1);
    assert_eq!(ring.charged(), one);
    assert_eq!(
        ring.nearest(&quiet_log(), Tick(5))
            .map(|(_, kept)| kept.count),
        Some(2)
    );
}

#[test]
fn discarding_from_a_tick_drops_that_tick_too() {
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 32);
    for tick in 10..20 {
        assert!(ring.keep(
            &quiet_log(),
            Tick(tick),
            &state(i64::try_from(tick).unwrap())
        ));
    }

    ring.discard_from(Tick(15));
    let kept: Vec<u64> = ring.ticks().map(|tick| tick.0).collect();
    // Fifteen goes. A rollback names the tick it is rolling back to and every
    // state from there on is about to be recomputed, so a ring that kept the
    // boundary would be holding one of them for nothing.
    assert_eq!(kept, [10, 11, 12, 13, 14]);
    assert_eq!(ring.charged(), one * 5);

    // And it is idempotent, so a caller that discards twice loses nothing more.
    ring.discard_from(Tick(15));
    assert_eq!(ring.len(), 5);
}

#[test]
fn discarding_from_before_everything_empties_the_ring() {
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 32);
    for tick in 10..14 {
        ring.keep(&quiet_log(), Tick(tick), &state(1));
    }
    ring.discard_from(Tick::ZERO);
    assert!(ring.is_empty());
    assert_eq!(ring.charged(), 0);
}

#[test]
fn clearing_empties_the_ring_and_the_budget_with_it() {
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 32);
    for tick in 0..6 {
        ring.keep(&quiet_log(), Tick(tick), &state(1));
    }
    ring.clear();
    assert!(ring.is_empty());
    assert_eq!(ring.charged(), 0);
}

#[test]
fn nothing_is_charged_for_a_state_that_is_no_longer_held() {
    // The accounting, checked against itself. Twenty states offered to a ring
    // with room for four leaves the ring charging for what it is holding and
    // not for what it has seen.
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 4);
    for tick in 0..20 {
        ring.keep(&quiet_log(), Tick(tick), &state(1));
    }
    assert_eq!(ring.charged(), one * ring.len());
    assert!(ring.charged() <= ring.budget());
    // And the ring did fill and evict rather than refusing everything after the
    // fourth, which is what would make the equality above true for the wrong
    // reason.
    assert_eq!(ring.len(), 4);
    assert!(ring.ticks().any(|tick| tick == Tick(19)));
}

#[test]
fn eviction_thickens_towards_the_present() {
    // What the spread is for, in the numbers it decides. Snapshots are offered
    // every tick for five hundred ticks into a ring of four kibibytes, and then
    // the shape is read twice over: as the gaps between what it kept, and as
    // what two seeks would cost.
    //
    // An even spread is what this eviction rule has to be told apart from, and
    // it is worth doing the arithmetic before reading the assertions: a couple
    // of dozen states spread evenly over five hundred ticks puts every gap near
    // twenty, which satisfies the last assertion below and none of the other
    // three.
    let session = play(500);
    let mut ring: Snapshots<Counter> = Snapshots::new(1 << 12);
    let mut current = session.opening.origin();
    for tick in 0..=500 {
        ring.keep(&quiet_log(), Tick(tick), &*current);
        if tick < 500 {
            let (next, _) = session.seek(&mut ring, Tick(tick + 1)).unwrap();
            current = next;
        }
    }

    let kept: Vec<u64> = ring.ticks().map(|tick| tick.0).collect();
    assert!(
        kept.len() >= 8,
        "a ring of {} has no spread to read",
        kept.len()
    );

    // The newest is there, so the forward path is one tick from a snapshot.
    assert_eq!(kept.last().copied(), Some(500));

    // Something old is there, so a seek into the first half does not start at
    // the opening. Tick zero is the opening and does not count as old.
    let old = kept.iter().find(|&&tick| tick > 0 && tick < 250);
    assert!(
        old.is_some(),
        "nothing between the opening and tick 250: {kept:?}"
    );

    // The gaps, which is the shape itself. The newest pair is one tick apart
    // and the first half of the session is thinned to gaps many times that; an
    // even spread makes the two numbers equal.
    let newest = kept
        .windows(2)
        .last()
        .map(|pair| pair[1] - pair[0])
        .expect("two snapshots make a gap");
    let oldest = kept
        .windows(2)
        .filter(|pair| pair[1] <= 250)
        .map(|pair| pair[1] - pair[0])
        .max()
        .expect("something is kept in the first half");
    assert_eq!(newest, 1, "the newest gap is {newest} ticks, from {kept:?}");
    assert!(
        newest * 16 <= oldest,
        "the newest gap is {newest} ticks and the widest below 250 is {oldest}, from {kept:?}",
    );

    // And the same shape priced as re-simulation, which is what a caller feels.
    let replay = |to: u64| Tick(to).since(ring.nearest(&quiet_log(), Tick(to)).unwrap().0);
    assert_eq!(
        replay(495),
        0,
        "a seek five ticks behind the present replayed {} ticks, from {kept:?}",
        replay(495),
    );
    assert!(
        replay(250) * 4 <= 250,
        "a seek to 250 replays {} of the 250 ticks the opening would cost, from {kept:?}",
        replay(250),
    );
}

#[test]
fn a_ring_with_no_budget_keeps_nothing_and_costs_nothing() {
    let mut ring: Snapshots<Counter> = Snapshots::new(0);
    for tick in 0..50 {
        assert!(!ring.keep(&quiet_log(), Tick(tick), &state(1)));
    }
    assert!(ring.is_empty());
    assert_eq!(ring.charged(), 0);
    assert_eq!(ring.nearest(&quiet_log(), Tick(49)), None);
}

#[test]
fn nearest_never_looks_forward() {
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 8);
    ring.keep(&quiet_log(), Tick(10), &state(10));
    ring.keep(&quiet_log(), Tick(20), &state(20));

    assert_eq!(ring.nearest(&quiet_log(), Tick(9)), None);
    assert_eq!(
        ring.nearest(&quiet_log(), Tick(10)).map(|(at, _)| at),
        Some(Tick(10))
    );
    assert_eq!(
        ring.nearest(&quiet_log(), Tick(19)).map(|(at, _)| at),
        Some(Tick(10))
    );
    assert_eq!(
        ring.nearest(&quiet_log(), Tick(20)).map(|(at, _)| at),
        Some(Tick(20))
    );
    assert_eq!(
        ring.nearest(&quiet_log(), Tick(9_999)).map(|(at, _)| at),
        Some(Tick(20))
    );
}

#[test]
fn the_ring_holds_states_and_not_openings() {
    // A guard on the fixture rather than on the ring: these tests measure a
    // budget in states, so a state that encoded to nothing would make every
    // assertion above about the per-entry overhead alone.
    let bytes = corvid_wire::encode(&opening().origin).unwrap().len();
    assert!(bytes >= 8, "the fixture state encodes to {bytes} bytes");
}

#[test]
fn a_larger_state_is_charged_more_than_a_smaller_one() {
    // The budget is a byte count and not a slot count, so what a state costs
    // has to depend on the state. A ring that charged `size_of` alone would
    // charge these two the same — a `Vec` is three words whatever is in it —
    // and a budget in bytes would be a budget in slots wearing a unit.
    let small = state(1);
    let large = State {
        count: 1,
        folded: 0,
        movers: vec![corvid_behavior::PlayerId(0); 200],
        roster: Vec::new(),
    };
    // The two hundred extra movers are two hundred extra bytes, one varint each,
    // and that difference is what a byte budget is charging for. Saying it as a
    // difference rather than as a ratio keeps it a statement about the payload
    // rather than about how large the per-entry overhead happens to be.
    assert!(
        charged_for(&large) >= charged_for(&small) + 200,
        "{} against {}",
        charged_for(&large),
        charged_for(&small),
    );

    // And a ring with room for several small states has room for none of the
    // large one, which is the consequence that matters.
    let mut ring: Snapshots<Counter> = Snapshots::new(charged_for(&small) * 3);
    assert!(ring.keep(&quiet_log(), Tick::ZERO, &small));
    assert!(!ring.keep(&quiet_log(), Tick(1), &large));
}

#[test]
fn a_state_evicted_by_its_own_arrival_is_not_kept() {
    // The third of the three reasons `keep` answers `false`, which is the one
    // that is not about the budget refusing the state outright: it fitted, it
    // went in, and the eviction it triggered chose it.
    //
    // Eviction scores an interior entry by the gap its removal would leave,
    // discounted by its age, so a state offered *between two close neighbours
    // far in the past* is the cheapest thing in the ring the moment it arrives.
    // A rewind that scrubbed backwards into a dense old stretch is exactly that
    // shape, and a caller that read the answer as "it is in there now" would go
    // looking for it.
    let one = charged_for(&state(1));
    let mut ring: Snapshots<Counter> = Snapshots::new(one * 4);
    for tick in [0, 100, 102, 1_000] {
        assert!(ring.keep(&quiet_log(), Tick(tick), &state(1)));
    }
    assert_eq!(ring.len(), 4);

    // Tick 101 sits between 100 and 102, nine hundred ticks behind the newest.
    assert!(!ring.keep(&quiet_log(), Tick(101), &state(9)));

    // And the answer is the truth: the ring is holding what it held before, and
    // nothing of the offered state is left charged against the budget.
    let kept: Vec<u64> = ring.ticks().map(|tick| tick.0).collect();
    assert_eq!(kept, [0, 100, 102, 1_000]);
    assert_eq!(ring.charged(), one * 4);

    // The neighbouring case, so the assertion above is about *which* state was
    // evicted rather than about a ring that refuses everything once full: the
    // same state offered at the newest end is kept, and something else goes.
    assert!(ring.keep(&quiet_log(), Tick(1_001), &state(9)));
    assert!(ring.ticks().any(|tick| tick == Tick(1_001)));
}
