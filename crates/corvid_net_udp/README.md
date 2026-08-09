# `corvid_net_udp`

A real socket behind the transport contract: one [`UdpNet`] wraps a single
non-blocking `UdpSocket`, and [`reliable`] builds the ordering and the
retransmission that [`corvid_net::Channel`] promises on top of datagrams that
have neither.

[`corvid_net_mock`](https://docs.rs/corvid_net_mock) proves a session survives
a link that lies. This proves it survives a link that is somewhere else. The
runtime above cannot tell the two apart, which is the whole reason
[`corvid_net::Transport`] is a trait.

```rust
use std::time::{Duration, Instant};

use corvid_net::{Delivery, PeerId, Transport as _};
use corvid_net_udp::UdpNet;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let here = UdpNet::bind(("127.0.0.1", 0), PeerId(1))?;
let there = UdpNet::bind(("127.0.0.1", 0), PeerId(2))?;
here.connect(PeerId(2), there.local()?)?;
there.connect(PeerId(1), here.local()?)?;

// Greetings are exchanged by polling, so both ends poll until each has the
// other on its roster.
let deadline = Instant::now() + Duration::from_secs(5);
while !(here.peers().contains(PeerId(2)) && there.peers().contains(PeerId(1))) {
    assert!(Instant::now() < deadline, "the pair never greeted");
    here.poll(&mut |_, _| {});
    there.poll(&mut |_, _| {});
}

here.send_datagram(PeerId(2), b"pong")?;

let mut heard = Vec::new();
let deadline = Instant::now() + Duration::from_secs(5);
while heard.is_empty() {
    assert!(Instant::now() < deadline, "the datagram never arrived");
    here.poll(&mut |_, _| {});
    there.poll(&mut |_, delivery| {
        if let Delivery::Datagram(bytes) = delivery {
            heard.push(bytes.to_vec());
        }
    });
}

assert_eq!(heard, vec![b"pong".to_vec()]);
# Ok(())
# }
```

## How it works

One socket, a table of peers by address, and one [`reliable`] channel pair per
peer per [`corvid_net::Channel`]. Nothing runs on a thread of its own:
[`UdpNet::poll`] is where packets are read, acknowledgements are sent and
retransmissions go out, and a runtime calls it once a tick anyway.

Every packet opens with a four-byte magic and a version, so a stray packet on
the port is dropped rather than parsed, and two builds that disagree about the
framing fail to talk rather than half-talk. A peer is greeted until it answers
and considered gone once it has said nothing for ten seconds; both edges reach
the caller as [`corvid_net::Delivery::Joined`] and
[`corvid_net::Delivery::Lost`], in the order they happened.

[`reliable`] is a stop-and-go window. Frames go out with consecutive sequence
numbers, the far end acknowledges the newest run it has complete, and anything
unacknowledged goes again after a fixed interval. It is free of sockets and of
clocks -- time arrives as an argument -- which is what lets the interesting
half be tested by dropping packets on purpose rather than by hoping a loopback
link loses one.

## Scope

This is a transport for a game on a local network, or for two processes on one
machine. It is deliberately not QUIC, and the gap is worth stating plainly.

There is no encryption and no authentication: anything that can reach the port
can send a packet, and a packet that parses is acted on. What limits the damage
sits above, where a lockstep peer refuses a datagram naming a tick past its
horizon, drops one it cannot decode, and halts on a contradiction, so the worst
a stranger achieves is what a broken link achieves. There is no congestion
control, only a fixed retry interval and a cap on how much may be in flight, so
on a genuinely congested path this would be one of the flows making it worse.
There is no path MTU discovery: [`corvid_net::DATAGRAM_LIMIT`] is assumed to
fit, which holds on every path carrying the conservative internet number of
1280 bytes. There is no NAT traversal, so each end is told where the other is.

For the internet, `quinn` behind this same trait is the answer, and the day it
lands nothing above the trait changes.
