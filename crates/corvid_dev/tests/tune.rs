#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

//! A tunable produces a `Proposal` and never a mutation, and the slider is one
//! `seek`.

use std::sync::Arc;

use corvid_dev::{Inspect, Invalid, Proposal, Rows, Slider, Tunable, Tuning};

use corvid_behavior::{Command, Level, Malformed, Player, ProfileId, Source, State};
use corvid_fixed::I16F16;
use corvid_hash::digest;
use corvid_replay::{Opening, Profile, Schema, Seed, Session, Snapshots, Unreachable};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// The tuning every peer has to agree on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Rules {
    step: I16F16,
    reach: I16F16,
}

/// A level with nothing in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Nowhere;

impl Level for Nowhere {
    type Reference = String;
    fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> {
        Ok(Self)
    }
}

/// A counter, so the state at tick `T` is arithmetic rather than a fixture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Counter(i64);

impl State for Counter {
    const NAME: &'static str = "counter";

    type Level = Nowhere;
    type Rules = Rules;
    type Action = ();

    fn tick(
        self,
        _level: &Nowhere,
        _players: &[Player<'_, ()>],
        rules: &Rules,
        _command: &mut impl Command<Reference = String>,
    ) -> Self {
        Self(self.0 + i64::from(rules.step.to_bits()))
    }
}

impl Inspect for Counter {
    fn inspect(state: &Self, out: &mut Rows) {
        let state = &state.0;
        out.field("count", state);
        {
            let mut digits = out.group("digits", 3);
            digits.field("hundreds", state / 100 % 10);
            digits.field("tens", state / 10 % 10);
            digits.field("units", state % 10);
        }
        out.field("negative", *state < 0);
    }
}

const STEP: I16F16 = I16F16::from_bits(1);

const fn rules() -> Rules {
    Rules {
        step: STEP,
        reach: I16F16::ONE,
    }
}

fn tuning() -> Tuning<Rules> {
    let mut tuning = Tuning::new();
    tuning.register(Tunable::new(
        "tower.arc.reach",
        I16F16::ZERO..=I16F16::from_f64(500.0),
        |rules: &Rules| rules.reach,
        |rules: &mut Rules, to| rules.reach = to,
    ));
    tuning.register(Tunable::new(
        "sim.step",
        I16F16::ZERO..=I16F16::ONE,
        |rules: &Rules| rules.step,
        |rules: &mut Rules, to| rules.step = to,
    ));
    tuning
}

fn opening() -> Opening<Counter> {
    Opening {
        level: "the only one".to_owned(),
        content: Arc::new(Nowhere),
        rules: Arc::new(rules()),
        roster: vec![Profile {
            account: ProfileId(1),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(0),
        first: Tick::ZERO,
        origin: Some(Arc::new(Counter(0))),
        schema: Schema::new("counter").field("State", "i64").digest(),
    }
}

#[test]
fn a_value_outside_the_range_is_refused_and_the_range_is_named() {
    let tuning = tuning();
    let live = rules();

    let refused = tuning
        .propose(&live, "tower.arc.reach", I16F16::from_f64(900.0), Tick(0))
        .unwrap_err();

    assert_eq!(
        refused,
        Invalid::OutOfRange {
            path: "tower.arc.reach",
            low: I16F16::ZERO,
            high: I16F16::from_f64(500.0),
            given: I16F16::from_f64(900.0),
        },
    );
}

#[test]
fn an_unregistered_path_is_refused() {
    let tuning = tuning();
    let live = rules();

    assert_eq!(
        tuning.propose(&live, "tower.arc.damage", I16F16::ONE, Tick(0)),
        Err(Invalid::Unknown {
            path: "tower.arc.damage".to_owned(),
        }),
    );
}

#[test]
fn proposing_never_touches_the_rules_it_was_handed() {
    let tuning = tuning();
    let live = rules();
    let before = digest(&live);

    let proposal = tuning
        .propose(&live, "tower.arc.reach", I16F16::from_f64(12.5), Tick(40))
        .expect("in range");

    assert_eq!(digest(&live), before);
    assert_eq!(live, rules());
    assert_eq!(
        proposal,
        Proposal {
            rules: Rules {
                step: STEP,
                reach: I16F16::from_f64(12.5),
            },
            because: "tower.arc.reach",
            at: Tick(40),
        },
    );
}

#[test]
fn a_proposal_reaches_the_hash() {
    let tuning = tuning();
    let live = rules();

    let proposal = tuning
        .propose(&live, "tower.arc.reach", I16F16::from_f64(12.5), Tick(40))
        .expect("in range");

    // Which is why every peer has to accept it: the digest of `Rules` is part
    // of what two peers compare.
    assert_ne!(digest(&proposal.rules), digest(&live));
    assert_eq!(digest(&proposal.into_rules()), {
        let mut applied = live;
        applied.reach = I16F16::from_f64(12.5);
        digest(&applied)
    });
}

#[test]
fn the_registry_reads_and_lists_in_order() {
    let tuning = tuning();
    let live = rules();

    assert_eq!(
        tuning.paths().collect::<Vec<_>>(),
        ["sim.step", "tower.arc.reach"],
    );
    assert_eq!(tuning.read(&live, "sim.step"), Some(STEP));
    assert_eq!(tuning.read(&live, "nothing"), None);
    assert_eq!(tuning.len(), 2);
}

/// One session, seeked to every tick it covers, from two differently sized
/// rings.
#[test]
fn seeking_reaches_the_state_the_run_recorded() {
    let mut session = Session::new(opening()).expect("one seat");
    session.log.extend_to(Tick(49)).expect("room for fifty");

    assert_eq!(Slider::range(&session), Tick::ZERO..=Tick(50));

    let step = i64::from(STEP.to_bits());
    let mut generous = Snapshots::<Counter>::new(1 << 20);
    let mut mean = Snapshots::<Counter>::new(0);

    for tick in 0_u64..=50 {
        let expected = step * i64::try_from(tick).expect("fifty fits");
        let (from_generous, _replayed) =
            Slider::seek(&session, &mut generous, Tick(tick)).expect("inside the log");
        let (from_mean, _replayed) =
            Slider::seek(&session, &mut mean, Tick(tick)).expect("inside the log");

        assert_eq!(from_generous.0, expected);
        assert_eq!(digest(&from_mean), digest(&expected));
    }
}

#[test]
fn seeking_outside_the_log_is_unreachable() {
    let mut session = Session::new(opening()).expect("one seat");
    session.log.extend_to(Tick(49)).expect("room for fifty");
    let mut snapshots = Snapshots::<Counter>::new(1 << 20);

    assert_eq!(
        Slider::seek(&session, &mut snapshots, Tick(51)),
        Err(Unreachable::After {
            to: Tick(51),
            last: Tick(50),
        }),
    );
}

#[test]
fn a_slider_clamps_into_what_the_session_reaches() {
    let session = Session::new(opening()).expect("one seat");

    assert_eq!(Slider::new(Tick(900)).clamped(&session), Tick::ZERO);
    assert_eq!(Slider::from(Tick(0)).clamped(&session), Tick::ZERO);
    assert!(!Slider::default().held);
}

#[test]
fn inspector_rows_are_in_declaration_order_and_a_group_reports_its_count() {
    let rows = Rows::of::<Counter>(&Counter(123));

    let names: Vec<&str> = rows.rows().iter().map(|row| row.name).collect();
    assert_eq!(
        names,
        ["count", "digits", "hundreds", "tens", "units", "negative"],
    );

    assert_eq!(rows.rows()[1].count, Some(3));
    assert_eq!(rows.rows()[2].depth, 1);
    assert_eq!(rows.rows()[4].depth, 1);
    // The group closed, so the row after it is back at the outer level.
    assert_eq!(rows.rows()[5].depth, 0);
    assert_eq!(rows.rows()[0].value, "123");
    assert_eq!(rows.rows()[5].value, "false");
}

#[test]
fn an_overlay_carries_the_digest_of_the_state_it_names() {
    let overlay = corvid_dev::Overlay::of(&7_i64, Tick(40));

    assert_eq!(overlay.digest, digest(&7_i64));
    assert_eq!(overlay.tick, Tick(40));
    assert!(overlay.line().starts_with("tick 40  "));
}
