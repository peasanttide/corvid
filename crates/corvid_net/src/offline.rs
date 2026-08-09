//! The transport that is not one.

use std::sync::LazyLock;

use corvid_signal::{Watch, channel};

use crate::{Channel, Delivery, PeerId, PeerSet, SendError, Transport};

/// The roster nobody is in. Built once and never published to, because nothing
/// can join a network that does not exist.
///
/// Labelled `"peers"` like every other roster signal in the workspace, and not
/// after this backend: a subscriber filtering a trace on the name the state has
/// in the program should find a single-player run in the same place it finds
/// every other.
// The `Emitter` is dropped here on purpose: a set that can never change is the
// whole of what this transport has to say about who is out there.
static NOBODY: LazyLock<Watch<PeerSet>> = LazyLock::new(|| channel("peers", PeerSet::new()).1);

/// A single machine, talking to nobody.
///
/// Every send, poll and roster answered honestly for a program that has no
/// network: an empty roster, nothing ever polled, and a send that fails with
/// [`SendError::Unknown`] because there is no peer of any number to reach.
/// [`datagram_limit`](Transport::datagram_limit) is left at the trait's
/// default, which is a fiction no datagram ever reaches.
///
/// It is what a single-player build runs on and what a test of everything
/// *above* the transport substitutes when the traffic is beside the point.
/// Neither wants a scheduler, a seed or a socket.
///
/// ```
/// use corvid_net::{Offline, PeerId, SendError, Transport};
///
/// fn play(link: &dyn Transport) -> usize {
///     for peer in &*link.peers().get() {
///         let _ = link.send_datagram(peer, b"my action for tick 41");
///     }
///     link.peers().get().len()
/// }
///
/// assert_eq!(play(&Offline), 0);
/// assert_eq!(
///     Offline.send_datagram(PeerId(1), b"anyone?"),
///     Err(SendError::Unknown(PeerId(1))),
/// );
/// ```
///
/// # Why a type of its own rather than `()`
///
/// `()` is the type of every statement-shaped call, so an impl on it would let
/// a call that returns nothing stand in for a transport:
/// `run(net.all(schedule))` where `run(net.endpoint(seat))` was meant would
/// type-check and run offline in silence. An alias is no help either, being a
/// spelling rather than a type.
///
/// A distinct type is what makes that a compile error, and the cost is one
/// import at the two or three places a program says it has no network:
///
/// ```compile_fail
/// use corvid_net::Transport;
///
/// fn run<T: Transport>(link: T) -> usize {
///     link.peers().get().len()
/// }
///
/// // `()` is not a transport, so a call that returns nothing is refused here
/// // rather than quietly standing in for one.
/// let _ = run(());
/// ```
///
/// # What it refuses with, and why
///
/// A caller that sends to the roster it was given never sees the error, since
/// the roster is empty -- the refusal is there for one that names a peer itself.
/// [`Unknown`](SendError::Unknown) and not [`Closed`](SendError::Closed):
/// nothing here is shutting down, and a caller that retries a `Closed` is
/// waiting on a recovery that is not coming. A transport whose roster happens
/// to be empty refuses the same way, which is what makes this substitutable
/// for one.
///
/// [`Transport::peers`] says a backend need never publish again, and this is
/// the backend that never does -- so a thread here cannot park on
/// [`Watch::blocking_wait`] and expect to be woken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offline;

impl Transport for Offline {
    fn send_datagram(&self, to: PeerId, _bytes: &[u8]) -> Result<(), SendError> {
        Err(SendError::Unknown(to))
    }

    fn send_stream(&self, to: PeerId, _channel: Channel, _bytes: &[u8]) -> Result<(), SendError> {
        Err(SendError::Unknown(to))
    }

    fn poll(&self, _sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {}

    fn peers(&self) -> &Watch<PeerSet> {
        &NOBODY
    }
}
