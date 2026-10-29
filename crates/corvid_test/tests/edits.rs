//! A run whose ticks edit and load its level: the runtime plays the changes,
//! records them in the session's timeline, and the capture replays to itself.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid_app::{Answer, App, Command as Asked};
use corvid_behavior::{Command, PlayerState, ProfileId};
use corvid_replay::{Opening, Profile, Schema, Seed};
use corvid_time::{Tick, TickSpan, Ticks};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Ramp {
    steps: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
#[error("a step of zero")]
struct Flat;

impl corvid_behavior::Level for Ramp {
    type Error = Flat;
    type Edit = u8;

    fn load(_name: &str) -> Result<Self, Flat> {
        Ok(Self::default())
    }

    fn edit(&self, edit: &u8) -> Result<Self, Flat> {
        if *edit == 0 {
            return Err(Flat);
        }
        let mut steps = self.steps.clone();
        steps.push(*edit);
        Ok(Self { steps })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Walk {
    now: u64,
    total: u64,
    seen: usize,
}

impl corvid_behavior::State for Walk {
    const NAME: &'static str = "walk";

    type Level = Ramp;
    type Rules = ();
    type Action = u8;

    fn load_level(self, _old: Option<&Ramp>, new: &Ramp) -> Self {
        Self {
            seen: new.steps.len(),
            ..self
        }
    }

    fn tick(
        self,
        level: &Ramp,
        _players: &[PlayerState<u8>],
        (): &(),
        command: &mut impl Command<u8>,
    ) -> Self {
        if self.now > 0 && self.now.is_multiple_of(10) {
            command.edit(u8::try_from(self.now / 10).unwrap_or(1));
        }
        if self.now == 35 {
            command.load("flat");
        }
        let height: u64 = level.steps.iter().map(|s| u64::from(*s)).sum();
        Self {
            now: self.now + 1,
            total: self.total + height,
            seen: self.seen,
        }
    }
}

corvid_app::game! {
    struct Walking;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Walk;
}

impl corvid_replay::Opens for Walk {
    fn opening() -> Opening<Self> {
        opening()
    }
}

fn opening() -> Opening<Walk> {
    Opening {
        level: "ramp".to_owned(),
        content: Arc::new(Ramp::default()),
        rules: Arc::new(()),
        roster: vec![Profile {
            account: ProfileId(1),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(3),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("walk").digest(),
    }
}

#[test]
fn a_run_that_edits_its_level_replays_to_itself() {
    let outcome = App::<Walking>::new()
        .opening(opening())
        .for_ticks(Ticks(60))
        .retain(corvid_app::Retention::Everything)
        .run()
        .expect("the run");
    let levels = &outcome.session.levels;
    assert_eq!(levels.at(Tick(21)).steps, vec![1, 2]);
    assert_eq!(
        levels.at(Tick(36)).steps,
        Vec::<u8>::new(),
        "flat was loaded"
    );
    assert_eq!(levels.current().steps, vec![4, 5]);
    assert_eq!(outcome.state.seen, 2);
    // The load was acted on, not reported as a gap.
    assert!(
        outcome
            .requests
            .iter()
            .filter(|r| matches!(r.command, Asked::Load(_)))
            .all(|r| r.answer == Answer::Done)
    );
    corvid_test::replays_to_itself(&outcome).expect("the capture replays to itself");
}
