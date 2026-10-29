//! What a tick asks the runtime for reaches it once, from the tick as it
//! finally happened: a peer that predicted past a remote seat's request still
//! hears it after the rollback, and a request only a prediction made is never
//! heard.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid_behavior::{Command, PlayerId, PlayerState, ProfileId};
use corvid_lockstep::{Budget, Peer};
use corvid_replay::{Opening, Profile, Schema, Seed, Session};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// A level with nothing in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Room;

#[derive(Debug, thiserror::Error)]
#[error("a room is never edited")]
struct Never;

impl corvid_behavior::Level for Room {
    type Error = Never;
    type Edit = u8;

    fn load(_name: &str) -> Result<Self, Never> {
        Ok(Self)
    }

    fn edit(&self, _edit: &u8) -> Result<Self, Never> {
        Err(Never)
    }
}

/// Counts ticks; a seat whose action is 9 asks for the lobby.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Ask {
    now: u64,
}

impl corvid_behavior::State for Ask {
    const NAME: &'static str = "ask";

    type Level = Room;
    type Rules = ();
    type Action = u8;

    fn load_level(self, _old: Option<&Room>, _new: &Room) -> Self {
        self
    }

    fn tick(
        self,
        _level: &Room,
        players: &[PlayerState<u8>],
        (): &(),
        command: &mut impl Command<u8>,
    ) -> Self {
        if players.iter().any(|player| player.action == 9) {
            command.lobby();
        }
        Self { now: self.now + 1 }
    }
}

/// A sink counting the lobby requests it hears.
#[derive(Debug, Default)]
struct Lobbies(u32);

impl Command<u8> for Lobbies {
    fn lobby(&mut self) {
        self.0 += 1;
    }
}

fn peer(seat: u16) -> Peer<Ask> {
    let opening = Opening::<Ask> {
        level: "room".to_owned(),
        content: Arc::new(Room),
        rules: Arc::new(()),
        roster: (1..=2)
            .map(|account| Profile {
                account: ProfileId(account),
                joined: Tick::ZERO,
                left: None,
            })
            .collect(),
        seed: Seed(5),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("ask").digest(),
    };
    Peer::new(
        Session::new(opening).unwrap(),
        PlayerId(seat),
        Budget::DEFAULT,
    )
}

/// Two peers play `turns`; seat 1 asks for the lobby on its turn `asks`
/// alone, and seat 0 hears seat 1 only every sixth turn, so it runs ahead
/// on predictions of seat 1 and rolls back. Answers how many lobby requests
/// each machine's runtime heard.
fn play(turns: u64, asks: u64) -> (u32, u32) {
    let mut here = peer(0);
    let mut there = peer(1);
    let (mut first, mut second) = (Lobbies::default(), Lobbies::default());
    let mut late = Vec::new();
    for turn in 0..turns {
        let _ = here.submit(0);
        let _ = there.submit(if turn == asks { 9 } else { 0 });
        let _ = there.receive(&here.outgoing());
        late.push(there.outgoing());
        if turn % 6 == 5 {
            for datagram in late.drain(..) {
                let _ = here.receive(&datagram);
            }
        }
        let _ = here.advance(&mut first);
        let _ = there.advance(&mut second);
    }
    assert!(here.tick() > Tick(100), "they barely played");
    (first.0, second.0)
}

#[test]
fn a_remote_request_reaches_a_peer_that_predicted_past_it() {
    // Seat 1's request lands while seat 0 is predicting it idle: seat 0
    // learns of it only through the rollback, and still hears it once.
    for asks in [18, 19, 20, 21, 22, 23] {
        assert_eq!(play(200, asks), (1, 1), "asked on turn {asks}");
    }
}

#[test]
fn a_request_only_a_prediction_made_is_never_heard() {
    // Prediction repeats a seat's last action, so seat 0 predicts seat 1's
    // one-off request again on the ticks after it; none of those happened,
    // and none reaches the runtime.
    let (here, there) = play(200, 40);
    assert_eq!((here, there), (1, 1));
}
