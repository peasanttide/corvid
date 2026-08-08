# `pong`

Two machines, one ball, and no authority between them.

Both peers simulate the whole game. Each one predicts what the other player has
not said yet by repeating their last action, and rolls back and re-simulates
when a real action disagrees with the guess. Every tick, each sends the other
its newest actions and a digest of its state; a digest that disagrees stops the
run and says which tick it started at. That is the whole of the netcode, and
none of it is in this crate — it is `corvid_lockstep` under
`corvid::App::transport`, and this game implements `State` and its four
client-local halves exactly as a single-seat game does.

```sh
cargo run -p pong                                    # one seat, a window
cargo run -p pong -- --bots 1                        # a window, and an opponent
cargo run -p pong -- --headless --spectator --bots 2 --ticks 900
cargo run -p pong -- --listen 9000 --connect 127.0.0.1:9001 --seat 0
cargo run -p pong -- --listen 9001 --connect 127.0.0.1:9000 --seat 1
```

The last two are two operating-system processes exchanging UDP. `W`/`S` or the
arrows move your paddle; seat 0 defends the left and seat 1 the right. There is
no flag list of this game's own: every one of those is
[`corvid::Arguments`](../../crates/corvid_app/src/cli.rs), and the whole of
`src/main.rs` is the five types this game is and how long its tick lasts.

Bots and a peer are refused together, on purpose. A controller is no part of
what a session records, so a machine that filled a seat locally while another
machine was in the same session would be writing a column the other one writes
differently.

The two-process pair with `--headless` is the same session with nobody watching
it: two processes, two sockets, and the same digest printed by each.

```text
$ pong --headless --ticks 300 --seat 0 --listen 9600 --connect 127.0.0.1:9601
0x4f7dfdb0b8c29eb7
… corvid_app::cli: the run ended tick=302 settled=282 digest=0x4f7dfdb0b8c29eb7
… corvid_app::cli: what the link cost heard=299 sent=300 rollbacks=0 …

$ pong --headless --ticks 300 --seat 1 --listen 9601 --connect 127.0.0.1:9600
0x4f7dfdb0b8c29eb7
… corvid_app::cli: the run ended tick=302 settled=282 digest=0x4f7dfdb0b8c29eb7
… corvid_app::cli: what the link cost heard=300 sent=299 rollbacks=0 …
```

Three things in that are worth reading twice. **One number on stdout**, so
`pong --headless | …` reads a line and compares it; everything else is a
`tracing` event on stderr, which is where the whole framework reports and which
`RUST_LOG` turns up. **The digest is at tick 282 rather than 302**: a run stops
when it stops, and the newest few ticks of each peer's state were simulated
partly from a guess about what the other player did, so the number two machines
can be held to is one from below the confirmed line. And **the two peers printed
the same number**, which is the whole of what this pair of commands is here to
show: a datagram this workspace built survived a round trip through the operating
system, both machines wrote the same rows into the same session, and both hashed
the same state at the same tick.

What it is **not** is a demonstration of prediction. `rollbacks=0` on both sides,
and it could not be anything else: a headless run has nobody at a keyboard, so
both seats submit `Move::Still` every tick, and repeating a seat's last action
guesses that right every time. A paddle that changes direction is what makes a
prediction wrong, and there is no way to put one on a networked seat from a
command line — bots are refused alongside a peer, for the reason above. Rollback
and its cost are the next section, where the paddles chase the ball and the link
is allowed to lie.

## What the netcode lab measures

[`Match`](src/rally.rs) is two peers, a `MockNet` and a `for` loop, with no
clock, no window and no thread in it — so a session over a link that loses a
third of its packets is decided entirely by the seed. `tests/session.rs` is that
with assertions on. Over the three named curves `corvid_net` ships, 900 ticks
each, seed `0xf1e2d3c`:

```text
link        ticks  confirmed  heard  rollbacks  deepest  replayed  stalls  agree
perfect      900        900    899          0        0         0       0  yes
domestic     900        899    883          0        0         0       0  yes
mobile       900        896    842         60        5       181       0  yes
```

Read it as the argument for prediction. A perfect link confirms every tick
before it is simulated, so nothing is ever predicted wrongly and nothing rolls
back. A domestic link loses a few and delays a few, and stays inside the input
delay. A mobile link mispredicts constantly — and the last column is still
`yes`: the two peers' digests agree, tick for tick, over every tick both have
every action for. That column is the entire claim this example exists to make.

`stalls` is a peer declining to simulate because it is already
`Budget::ahead` past the tick every seat has confirmed. Stalling is a decision
rather than a failure: a visible hitch is better than predicting a decision
nobody has made.

## Why the action is a direction

```text
pub enum Move { Still, Up, Down }
```

Prediction repeats a seat's newest action, so a player holding `Up` for twenty
ticks is predicted right nineteen times and the tick they change direction is
the one that mispredicts. A paddle *position* on the wire would be right by
accident every tick, would need no prediction, and would make this example prove
nothing — it would also let a peer put its paddle anywhere, which is what
"nobody is the authority" costs you if the actions are not intents.

## The state

```text
pub struct Table {
    pub ball: Ball,
    pub paddles: [Paddle; 2],
    pub scores: [u16; 2],
    pub serve: u16,
    pub towards: bool,
    pub contact: Option<Contact>,
    pub now: Tick,
    pub over: Option<u8>,
}
```

Small enough to hold in your head while reading a rollback trace, which is why
the game under this netcode is pong. Everything in it is integer fixed point
and there is no randomness anywhere — a session's whole outcome is the two
action logs, so a desync report can only ever be about the arithmetic.

`contact` is what the sound and the flash are read out of. It is in the hashed
state rather than worked out by the client because a hit is a simulation event:
every peer agrees it happened, it survives a save, and a client that recomputed
it from two ball positions would have to guess.

```rust
use pong::{Move, court, opening, origin, rules};
use corvid::{Discard, State, behavior::{Player, Presence, PlayerId}};

// One tick, called directly: the paddles move and the ball waits to be served.
// The sink is a `Discard`, because this game's ticks ask for nothing.
let next = origin().tick(
    &court(),
    &[
        Player { id: PlayerId(0), presence: Presence::Active, action: &Move::Up },
        Player { id: PlayerId(1), presence: Presence::Active, action: &Move::Still },
    ],
    &rules(),
    &mut Discard::new(),
);

// Seat zero moved and seat one did not, and nothing asked the runtime for
// anything — this game's ticks never do.
assert!(next.paddles[0].at > origin().paddles[0].at);
assert_eq!(next.paddles[1].at, origin().paddles[1].at);
```

## The tests are the point

- `tests/table.rs` — the simulation: bounces, spin, scoring, and that the ball
  a frame draws is the two states exactly at the two ends of the interpolation.
- `tests/baseline.rs` — the digests this game had before the contracts changed.
  A failure there is the simulation having moved, whatever else it was meant to
  be.
- `tests/bot.rs` — that `Opponent`, the paddle `--bots N` fills a seat with, is
  actually trying: two of them rally for a minute and concede a handful of
  points, and they beat a paddle that follows the ball's current height. And
  that it moves the paddle of the seat it is *asked* for, since it holds no seat
  of its own and reading the wrong one would play a good paddle at the wrong end
  of the court.
- `tests/session.rs` — two peers over `MockNet` at all three curves: identical
  digest traces over every confirmed tick, rollbacks that measurably happen, a
  total outage that stalls both peers without desyncing them, and a doctored
  digest that is caught.
- `tests/linked.rs` — the same game through `corvid::App::transport`: two
  runtimes, two threads, two endpoints, and one agreed digest at the end.
- `tests/socket.rs` — two `UdpNet`s on loopback, which is two real sockets
  carrying a real session, and a peer with nobody at the far end that stalls
  rather than failing.
- `tests/together.rs` — that `rally::together`, which plays both seats in one
  process with a real peer on a thread, really is two peers: the mode hands its
  run back, and a session with somebody in it heard datagrams.
- `tests/drawn.rs` — an adapter actually rasterising this game's pipeline, and
  a picture read back and looked at.

`corvid_net`'s own `tests/reliable.rs` is where packets are dropped on purpose:
loopback loses nothing, so the retransmission and reassembly under
`send_stream` are driven directly rather than hoped for. And
`corvid`'s `tests/transfer.rs` is where a machine that cannot catch up from
actions at all is handed a whole state — scripted rather than provoked, because
an outage that stalls everybody stalls nobody's head and the window then covers
the gap by itself.

## What this example is not

It is not a client-server game: there is no authority, no reconciliation of a
client's guess against a server's truth, and no interpolation of somebody else's
paddle. Every peer computes the same state and they check that they did.

It is not matchmaking. Both peers start from the same `Opening` because they are
the same binary started twice, and each is told where the other is.

**Closing one window does not freeze the other player.** Stop one of the two
processes above mid-game and the survivor waits out the socket's patience, agrees
with itself that the seat has gone — a departure is a tick written into the
roster, so it is part of the session rather than one machine's opinion — and
plays on to the end. Started with `--ticks 200` against a survivor on
`--ticks 700`, that is what the survivor says:

```text
… corvid_app::net: a peer went away; its seat submits nothing from here on
… corvid_app::net: every machine still here agreed this seat left seat=0 at=210 rewound=0
… corvid_app::cli: the run ended tick=702 settled=682 digest=0x06a6c78cd9fee1ed
```

It does not seat a player who comes back. A machine that reconnects is handed a
state and follows the session, because a departure is written into the roster
and a roster records one spell per seat — taking a seat back is a mid-session
*join*, which is milestone 7's work and needs a game with a roster in its state.
What is here is the half a two-player game needs: nobody is left waiting for a
window that closed.
