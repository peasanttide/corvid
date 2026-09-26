//! A session's level changes by its ticks' edits and loads: stepping applies
//! them in order, refuses what the level refuses, and a seek, a rebuild and a
//! session that forgot its past all agree with the session as it was played.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid_behavior::{Command, Discard, PlayerId, PlayerState, ProfileId};
use corvid_replay::{Opening, Profile, Schema, Seed, Session, Snapshots, Timeline};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// A ramp of steps, one pushed by each edit; an edit of zero is refused, and
/// the level called "flat" has no steps.
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

/// Edits every tenth tick (a zero, refused, at tick 30), loads "flat" at
/// tick 45, and sums the ramp's height as it walks.
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
            let step = if self.now == 30 {
                0
            } else {
                u8::try_from(self.now / 10).unwrap()
            };
            command.edit(step);
        }
        if self.now == 45 {
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

fn session() -> Session<Walk> {
    Session::new(Opening::<Walk> {
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
    })
    .unwrap()
}

/// Plays `ticks` ticks the way a runtime does: one shared step a tick, the
/// timeline and the marks kept.
fn played(ticks: u64) -> (Session<Walk>, Walk) {
    let mut session = session();
    let mut state = Walk::default();
    for at in 0..ticks {
        let at = Tick(at);
        session.log.extend_to(at).unwrap();
        session.log.set(at, PlayerId(0), 0).unwrap();
        let players = corvid_replay::players(&session.opening, at, |_| Some(0));
        let level = Arc::clone(session.levels.at(at));
        let stepped = corvid_replay::step(state, &level, &players, &(), &mut Discard::new());
        let mark = corvid_replay::mark(&stepped.state, stepped.changed.as_ref().map(|(l, _)| &**l));
        if let Some((level, changes)) = stepped.changed {
            session.levels.push(at.next(), level, changes);
        }
        session.marks.push(mark);
        state = stepped.state;
    }
    (session, state)
}

#[test]
fn edits_apply_in_order_the_level_refuses_what_it_refuses_and_a_load_replaces_it() {
    let (session, state) = played(60);
    let levels = &session.levels;
    assert_eq!(levels.at(Tick(10)).steps, Vec::<u8>::new());
    assert_eq!(levels.at(Tick(11)).steps, vec![1]);
    assert_eq!(levels.at(Tick(25)).steps, vec![1, 2]);
    // Tick 30's zero was refused: nothing changed, nothing recorded.
    assert_eq!(levels.at(Tick(31)).steps, vec![1, 2]);
    assert_eq!(levels.at(Tick(41)).steps, vec![1, 2, 4]);
    // "flat" was loaded after tick 45; the edit at 50 builds on it.
    assert_eq!(levels.at(Tick(46)).steps, Vec::<u8>::new());
    assert_eq!(levels.at(Tick(51)).steps, vec![5]);
    assert_eq!(state.seen, 1, "the state folded the last level in");
}

#[test]
fn a_seek_through_the_changes_arrives_where_the_session_did() {
    let (played, state) = played(60);
    let mut read = Session::<Walk>::load(&played.save().unwrap(), played.opening.schema).unwrap();
    let mut snapshots = Snapshots::new(1 << 16);
    let (sought, _) = read.seek(&mut snapshots, Tick(60)).unwrap();
    assert_eq!(*sought, state);
    assert_eq!(read.levels.current().steps, vec![5]);
    assert_eq!(read.levels.changes(), played.levels.changes());
}

#[test]
fn a_rebuild_from_the_changes_is_the_same_timeline() {
    let (played, _) = played(60);
    let rebuilt = Timeline::rebuild(Arc::clone(played.levels.origin()), played.levels.changes());
    assert_eq!(rebuilt, played.levels);
}

#[test]
fn a_session_that_forgets_its_past_opens_on_the_level_played_then() {
    let (mut played, _) = played(60);
    let state = Arc::new(Walk::default());
    played.forget_before(Tick(35), state).unwrap();
    assert_eq!(played.opening.content.steps, vec![1, 2]);
    assert_eq!(played.levels.at(Tick(41)).steps, vec![1, 2, 4]);
    // The origin and the history stay for a machine that has to rebuild.
    assert_eq!(played.levels.origin().steps, Vec::<u8>::new());
    assert_eq!(played.levels.changes().len(), 5);
}
