# `corvid_lockstep`

The simulation ring of a [Corvid](https://github.com/peasanttide/corvid)
session. A [`Peer`] predicts what has not arrived, rolls back when a real action
disagrees with the prediction, and exchanges a state digest with everyone else
every tick.

It never names a transport. A peer produces a [`Datagram`] and consumes one, and
carrying those bytes to another machine is the runtime's job — which is what
lets a whole session be driven with no network in the process at all, as the
example below and every test in this crate do.

Everything here is `no_std` with an allocator, integer-only and hashed, because
everything it computes is part of the simulation.

```rust
use std::sync::Arc;

use corvid_behavior::PlayerId;
use corvid_hash::digest;
use corvid_lockstep::{Budget, Peer};
use corvid_replay::Session;
use corvid_time::Tick;
use corvid_behavior::ProfileId;
use corvid_replay::{Opening, Profile, Schema, Seed};
# use corvid_behavior::{Command, Level as LevelContract, Player, State};
# use corvid_files::{Malformed, Source};
# use serde::{Deserialize, Serialize};
#
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Level { ceiling: i64 }
# impl LevelContract for Level {
#     type Reference = String;
#     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> {
#         Ok(Self { ceiling: 1_000 })
#     }
# }
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Rules { step: i64 }
# #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# struct Counter { count: i64, folded: u64 }
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
# enum Action { #[default] Idle, Bump }
# impl State for Counter {
#     const NAME: &'static str = "counter";
#     type Level = Level;
#     type Rules = Rules;
#     type Action = Action;
#     fn tick(
#         self,
#         level: &Level,
#         players: &[Player<'_, Action>],
#         rules: &Rules,
#         _command: &mut impl Command<Reference = String>,
#     ) -> Self {
#         let mut next = self;
#         for player in players {
#             next.folded = next.folded
#                 .wrapping_mul(0x0100_0000_01b3)
#                 .wrapping_add(u64::from(player.id.0))
#                 .wrapping_add(u64::from(matches!(player.action, Action::Bump)));
#             if matches!(player.action, Action::Bump) {
#                 next.count = (next.count + rules.step).min(level.ceiling);
#             }
#         }
#         next
#     }
# }
#
# fn seat(account: u64) -> Profile {
#     Profile { account: ProfileId(account), joined: Tick::ZERO, left: None }
# }
#
# fn opening() -> Opening<Counter> {
#     Opening {
#         level: "terminus".to_owned(),
#         content: Arc::new(Level { ceiling: 1_000 }),
#         rules: Arc::new(Rules { step: 3 }),
#         roster: vec![seat(1), seat(2)],
#         seed: Seed(0x5eed),
#         first: Tick::ZERO,
#         origin: None,
#         schema: Schema::new("counter").field("State.count", "i64").digest(),
#     }
# }

// Two seats, two machines, and nothing between them but these two values.
let mut here = Peer::new(Session::new(opening())?, PlayerId(0), Budget::DEFAULT);
let mut there = Peer::new(Session::new(opening())?, PlayerId(1), Budget::DEFAULT);

for tick in 0..20 {
    // Each machine's own action, submitted for `now + Budget::delay`.
    here.submit(if tick % 3 == 0 { Action::Bump } else { Action::Idle })?;
    there.submit(if tick % 5 == 0 { Action::Bump } else { Action::Idle })?;

    // The frames of bytes a transport would have carried. `here` predicts
    // `there`'s action for every tick this has not reached it for, and rolls
    // back on the first one it got wrong.
    let (from_here, from_there) = (here.outgoing(), there.outgoing());
    here.receive(&from_there)?;
    there.receive(&from_here)?;

    here.advance(&mut corvid_behavior::Discard::new())?;
    there.advance(&mut corvid_behavior::Discard::new())?;
}

// Two peers, one log, one state. The digests are the whole claim.
assert_eq!(here.tick(), there.tick());
assert_eq!(digest(here.state()), digest(there.state()));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Prediction repeats the last action

For every seat that has not confirmed a tick, the action simulated there is that
seat's newest confirmed action — for a tower defence, right almost always, since
a player who was idle stays idle and a player mid-build stays mid-build. When
the real action arrives and it is `!=` the prediction, that is a mispredict and
the rollback begins; when it is `==`, the log is confirmed and nothing is
re-simulated, because the state was already right.

That equality is why `Simulate::Action` is `Eq`: the check is one comparison,
not a digest.

## A rollback discards snapshots strictly after the corrected tick

The state *at* tick `T` is the result of simulating the rows *before* `T`, so a
correction to the row at `T` leaves the state at `T` untouched and invalidates
every state after it. [`Peer::receive`] therefore discards from `T.next()` and
restores from the snapshot at `T`.

Passing `T` is not the cautious version of that. Forward play keeps the state at
`S` before row `S` is written, so counting row `T` would take every entry the
ring ever holds and send every rollback back to the opening.

## The digest rides in the action datagram

One packet carries the actions *and* the mark, because a digest sent separately
is a second packet on a path that is already sending one every tick. Four rows
of redundancy — [`WINDOW`] — means a single loss needs no retransmission and a
burst of three still recovers on the next arrival; at fifteen ticks a second
that is 266 milliseconds of cover.

## A desync report names subsystems

A game implements [`Bisect`] and names its columns, and the report names them
back:

```text
desync at tick 4127, peer 2
  agreed through 4126
  state.creeps.velocity  differs   local 0x8f21… remote 0x8f20…
  state.creeps.position  agrees
  state.towers           agrees
  first divergent index  creep 30281, region 44
```

There is no reflection over a `State`, because reflection is a second
serialization format that can disagree with the first. A game that implements
nothing gets the default, which probes the whole state as one field and reports
that it differs — true, and much less useful.

[`bisect`] is behind the `dev` feature. Without it a desync reports and
resynchronises from a full state transfer, which is [`Peer::resync_request`] and
[`Peer::adopt`].
