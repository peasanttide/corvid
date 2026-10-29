//! Two peers edit their level while one of them is hearing the other late: a
//! rollback across the edits plays them again, and both machines end up with
//! the same marks and the same level.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid_behavior::{Command, Discard, PlayerId, PlayerState, ProfileId};
use corvid_lockstep::{Budget, Peer};
use corvid_replay::{Opening, Profile, Schema, Seed, Session};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// A ramp of steps, one pushed by each edit; an edit of zero is refused.
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

/// Walks the ramp: every tick adds the ramp's height, so a machine on the
/// wrong ramp walks a different sum.
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
        players: &[PlayerState<u8>],
        (): &(),
        command: &mut impl Command<u8>,
    ) -> Self {
        for player in players {
            if player.action != 0 {
                command.edit(player.action);
            }
        }
        let height: u64 = level.steps.iter().map(|s| u64::from(*s)).sum();
        Self {
            now: self.now + 1,
            total: self.total + height,
            seen: self.seen,
        }
    }
}

fn peer(seat: u16) -> Peer<Walk> {
    let opening = Opening::<Walk> {
        level: "ramp".to_owned(),
        content: Arc::new(Ramp::default()),
        rules: Arc::new(()),
        roster: (1..=2)
            .map(|account| Profile {
                account: ProfileId(account),
                joined: Tick::ZERO,
                left: None,
            })
            .collect(),
        seed: Seed(3),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("walk").digest(),
    };
    Peer::new(
        Session::new(opening).unwrap(),
        PlayerId(seat),
        Budget::DEFAULT,
    )
}

#[test]
fn a_rollback_across_edits_leaves_both_peers_on_the_same_level() {
    let mut here = peer(0);
    let mut there = peer(1);
    let mut late = Vec::new();
    for turn in 0..400_u64 {
        // Seat 1 edits at its turn 18, seat 0 at 20.
        let _ = here.submit(if turn == 20 { 7 } else { 0 });
        let _ = there.submit(if turn == 18 { 5 } else { 0 });
        let _ = there.receive(&here.outgoing());
        late.push(there.outgoing());
        // Seat 0 hears seat 1 six turns late for the first hundred turns.
        if turn >= 100 || turn % 6 == 5 {
            for datagram in late.drain(..) {
                let _ = here.receive(&datagram);
            }
        }
        let _ = here.advance(&mut Discard::new());
        let _ = there.advance(&mut Discard::new());
    }
    let reached = here.tick().min(there.tick());
    assert!(reached > Tick(200), "they barely played: {reached:?}");
    let mut compared = 0;
    for at in 1..reached.0.saturating_sub(20) {
        let (Some(a), Some(b)) = (
            here.session.marks.get(Tick(at)),
            there.session.marks.get(Tick(at)),
        ) else {
            continue;
        };
        assert_eq!(a, b, "the peers disagree at tick {at}");
        compared += 1;
    }
    assert!(compared > 150, "only {compared} ticks compared");
    let steps = |peer: &Peer<Walk>| peer.session.levels.at(Tick(150)).steps.clone();
    assert_eq!(steps(&here), vec![5, 7]);
    assert_eq!(steps(&there), vec![5, 7]);
    assert_eq!(
        here.session.levels.changes(),
        there.session.levels.changes()
    );
}
