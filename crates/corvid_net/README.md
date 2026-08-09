# `corvid_net`

The transport contract for [Corvid](https://github.com/peasanttide/corvid), and
nothing else: bytes to a peer, bytes from a peer, and a published roster.
[`Transport`] is the whole of the vocabulary, and there is no socket in this
crate.

```rust
use corvid_net::{Channel, Delivery, PeerSet, SendError, Transport};

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
unreliable and unordered, which is what the action stream wants -- see
[`Transport::send_datagram`]. **Streams** are reliable and ordered within a
[`Channel`].

## The backends

[`Offline`] is the one that ships here, and it is the honest empty one -- a
single machine talking to nobody, which is what a single-player build runs on
and what a test of everything above the transport substitutes. Its own docs say
what it answers.

It is a type of its own rather than `()`. `()` is the type of every
statement-shaped call, so an impl on it would let a call that returns nothing
stand in for a transport: `run(net.all(schedule))` where
`run(net.endpoint(seat))` was meant would type-check and run offline in
silence. An alias is no help either, being a spelling rather than a type. The
cost is one import at the two or three places a program says it has no
network.

Anything that carries a byte is a crate of its own, so a caller written against
this trait compiles neither a scheduler nor a socket it never names.

## The three decisions

[`poll`](Transport::poll) takes a sink rather than returning an iterator, every
method takes `&self`, and a [`PeerSet`] is sorted. Each says why where it is
declared, and the third is the one to know before reading any of the rest: a
roster built by iterating peers is a roster every peer must build identically,
and a `HashSet` would order it by its hash seed.
