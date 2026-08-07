# `corvid_net`

Transport for [Corvid](https://github.com/peasanttide/corvid), and nothing
else: bytes to a peer, bytes from a peer, and a published roster. It knows
nothing about ticks, actions or games — a frame of bytes goes in one end and
comes out the other, and what the bytes mean belongs to whatever built them.

```rust
use corvid_net::{Channel, Delivery, PeerId, PeerSet, SendError, Transport};
use corvid_signal::Watch;

/// What a peer does with a transport, whichever transport it was handed.
fn one_turn(link: &dyn Transport, everyone: &PeerSet) -> Result<(), SendError> {
    for peer in everyone {
        link.send_datagram(peer, b"my action for tick 41")?;
    }

    link.poll(&mut |from, what| match what {
        Delivery::Datagram(bytes) => drop((from, bytes)),
        Delivery::Stream { channel: Channel::Transfer, bytes } => drop(bytes),
        Delivery::Joined => drop(from),
        Delivery::Lost { because } => drop(because),
        _ => {}
    });

    Ok(())
}
```

Two ways to send, because a game has two kinds of traffic. **Datagrams** are
unreliable and unordered, which is what the action stream wants: a late packet
is worthless, since rollback has already covered for it. **Streams** are
reliable and ordered within a [`Channel`] and not across them, so a whole-state
transfer for a join does not hold up a chat line.

## `MockNet`

The transport this crate ships is `MockNet`: peers linked in-process, with
scriptable latency, jitter, loss and reorder. It is public API rather than a
test helper, and that is the point of the split — a netcode lab and a netcode
test are this one setup with different assertions, and a game downstream builds
its own netcode tests out of it.

Nothing is delivered by a thread. The clock moves only in `advance`, so a test
drives it a step at a time and a lab drives it from its frame loop.

```rust
use core::time::Duration;

use corvid_net::{Delivery, MockNet, PeerId, Schedule, Transport as _};

const STEP: Duration = Duration::from_millis(10);

/// Everything peer 1 hears while peer 0 sends it one datagram per step.
fn run(seed: u64) -> Vec<Vec<u8>> {
    let net = MockNet::new(2, seed);
    net.all(Schedule::MOBILE);

    let alice = net.endpoint(PeerId(0));
    let bob = net.endpoint(PeerId(1));

    let mut heard = Vec::new();
    for tick in 0..100_u32 {
        let _ = alice.send_datagram(PeerId(1), &tick.to_le_bytes());
        net.advance(STEP);
        bob.poll(&mut |_, what| {
            if let Delivery::Datagram(bytes) = what {
                heard.push(bytes.to_vec());
            }
        });
    }
    heard
}

// A bad mobile link: some of those hundred are gone, and some arrived out of
// the order they were sent in.
let once = run(0x5eed);
assert!(once.len() < 100);
assert!(once.windows(2).any(|pair| pair[0] > pair[1]));

// And every draw is a hash of `(seed, link, sequence)` rather than a system
// RNG or a clock, so the same seed loses the same packets in the same order.
assert_eq!(once, run(0x5eed));
assert_ne!(once, run(0xd1ce));
```

A `Schedule` is four numbers — `latency`, `jitter`, `loss`, `reorder` — and
three of them are named: `PERFECT`, `DOMESTIC` (40 ms, 10 ms, 1 %, 1 %) and
`MOBILE` (120 ms, 60 ms, 5 %, 5 %). Each direction of each link carries its own,
because an asymmetric link is the case worth modelling: a peer whose uplink is
the bad half is a peer whose actions arrive late while everyone else's arrive on
time.

Reliable traffic is reliable even on a lossy link. A stream frame that is lost
is tried again after twice the latency, and the frames behind it wait — so a
state transfer over a bad link takes a visible amount of time, which is what a
join actually feels like.

```rust
use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::{Channel, Delivery, MockNet, PeerId, Schedule, Transport as _};

let net = MockNet::new(2, 7);
net.all(Schedule::new(
    Duration::from_millis(50),
    Duration::ZERO,
    Factor16::from_f64(0.5),
    Factor16::ZERO,
));

let alice = net.endpoint(PeerId(0));
let bob = net.endpoint(PeerId(1));

for part in 0..8_u8 {
    alice.send_stream(PeerId(1), Channel::Transfer, &[part])?;
}

let mut whole = Vec::new();
for _ in 0..200 {
    net.advance(Duration::from_millis(10));
    bob.poll(&mut |_, what| {
        if let Delivery::Stream { bytes, .. } = what {
            whole.extend_from_slice(bytes);
        }
    });
}

// Half the attempts were dropped, and all eight parts still arrived in order.
assert!(net.tally().dropped > 0);
assert_eq!(whole, [0, 1, 2, 3, 4, 5, 6, 7]);
# Ok::<(), corvid_net::SendError>(())
```

## The four decisions

**`poll` takes a sink rather than returning an iterator.** An iterator would
borrow the transport for the length of the loop, and a peer that wants to answer
a packet from inside the loop could not. The sink is what lets the handler send.

**Every method takes `&self`,** so each backend carries its own interior
mutability. `&mut self` would put a `&mut dyn Transport` on every path that
wants to send — including from inside `poll`, which is the borrow that does not
work.

**A `PeerSet` is sorted, and that is load-bearing.** A roster built by iterating
peers is a roster every peer must build identically, and a `HashSet` would order
it by its hash seed. Sorted, in a `Vec`, and `Hash` derived over that order, so
two peers' rosters compare as values.

**Delivery is a priority queue keyed by `(due, sequence)`.** Two packets due at
the same instant deliver in send order, and the tie is never broken by a hash
map's iteration order.

## What `MockNet` does not model

The tail of a real network: no congestion collapse, no path MTU discovery, no
NAT rebinding, no handshake, no encryption. It models the four things that
change what a lockstep peer does.

It is also the one transport in this crate — there is no socket here, and
nothing in this crate opens one. A peer is written against the trait rather than
against `MockNet`, so a backend that does reach a real path is the same trait
with a different implementation behind it.

A `PeerId` is not a seat, either. A machine may hold two seats and a seat may
move between machines, so the mapping between a peer and a game's player
belongs to the runtime rather than to the transport.
