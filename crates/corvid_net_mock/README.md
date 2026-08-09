# `corvid_net_mock`

Peers linked in-process over a network that lies on purpose: scriptable
latency, jitter, loss and reorder. Each peer's [`Endpoint`] is a
[`corvid_net::Transport`], so a caller written against the trait runs on this
unchanged.

The clock moves only in [`MockNet::advance`] and nothing is delivered by a
thread, so a test drives it a step at a time.

```rust
use core::time::Duration;

use corvid_net::{Delivery, PeerId, Transport as _};
use corvid_net_mock::{MockNet, Schedule};

const STEP: Duration = Duration::from_millis(10);

/// Everything peer 2 hears while peer 1 sends it one datagram per step.
fn run(seed: u64) -> Vec<Vec<u8>> {
    let net = MockNet::new(2, seed);
    net.all(Schedule::MOBILE);

    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));

    let mut heard = Vec::new();
    for tick in 0..100_u32 {
        let _ = alice.send_datagram(PeerId(2), &tick.to_le_bytes());
        net.advance(STEP);
        bob.poll(&mut |_, what| {
            if let Delivery::Datagram(bytes) = what {
                heard.push(bytes.to_vec());
            }
        });
    }
    heard
}

// A bad mobile link: of the hundred, five are lost outright and thirteen are
// still in the air when the loop stops, since a floor of 120 ms is twelve
// steps deep. What did land came out of the order it was sent in.
let once = run(0x5eed);
assert!(once.len() < 100);
assert!(once.windows(2).any(|pair| pair[0] > pair[1]));

// And every draw comes from a `ChaCha8Rng` keyed by `(seed, link, sequence)`
// rather than from a system RNG or a clock, so the same seed loses the same
// packets in the same order.
assert_eq!(once, run(0x5eed));
assert_ne!(once, run(0xd1ce));
```

A [`Schedule`] is four numbers -- `latency`, `jitter`, `loss`, `reorder` -- and
three whole curves are named: `PERFECT`, `DOMESTIC` (40 ms, 10 ms, 1 %, 1 %)
and `MOBILE` (120 ms, 60 ms, 5 %, 5 %). Each direction of each link carries its
own.

Reliable traffic is reliable even on a lossy link: a lost stream frame is tried
again after twice the latency, the frames behind it wait, and a state transfer
over a bad link therefore takes a visible amount of time.

```rust
use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::{Channel, Delivery, PeerId, Transport as _};
use corvid_net_mock::{MockNet, Schedule};

let net = MockNet::new(2, 11);
net.all(Schedule::new(
    Duration::from_millis(50),
    Duration::ZERO,
    Factor16::from_f64(0.5),
    Factor16::ZERO,
));

let alice = net.endpoint(PeerId(1));
let bob = net.endpoint(PeerId(2));

for part in 0..8_u8 {
    alice.send_stream(PeerId(2), Channel::Transfer, &[part])?;
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

// Eight parts cost fourteen attempts, six of which were dropped and tried
// again, and all eight still arrived in order.
assert!(net.tally().dropped > 0);
assert_eq!(whole, [0, 1, 2, 3, 4, 5, 6, 7]);
# Ok::<(), corvid_net::SendError>(())
```

## What makes a run reproducible

Delivery is a priority queue keyed by `(due, sequence)`, so two packets due at
the same instant deliver in send order, and nothing else reaches the schedule
-- no thread, no clock, no hash map's iteration order.

The reproducibility is one driver's, though: [`MockNet`] is `Sync` with `&self`
methods, so two threads sending concurrently draw in lock-acquisition order.

## What this does not model

The tail of a real network: no congestion collapse, no path MTU discovery, no
NAT rebinding, no handshake, no encryption. It models the four things that
change what a peer rolling back has to survive.
