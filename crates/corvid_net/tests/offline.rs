//! `Offline`: the contract answered by a program with no network.

use corvid_net::{Channel, Offline, PeerId, PeerSet, SendError, Transport};
use corvid_signal::Seen;

#[test]
fn a_send_to_any_peer_at_all_is_unknown() {
    // Every number, not only the ones a roster might plausibly hold: there is
    // no peer table here to be absent from, so the refusal cannot depend on
    // which peer was named.
    for peer in [PeerId(0), PeerId(1), PeerId(9_999), PeerId::NONE] {
        assert_eq!(
            Offline.send_datagram(peer, b"anyone?"),
            Err(SendError::Unknown(peer))
        );
        for &channel in Channel::ALL {
            assert_eq!(
                Offline.send_stream(peer, channel, b"anyone?"),
                Err(SendError::Unknown(peer))
            );
        }
    }

    // Not `TooLarge`, even for a payload that is: the peer check comes first,
    // and a caller told to shrink a datagram nobody could have received would
    // shrink it and be told the same thing again.
    assert_eq!(
        Offline.send_datagram(PeerId(0), &vec![0; Offline.datagram_limit() + 1]),
        Err(SendError::Unknown(PeerId(0)))
    );
}

#[test]
fn nothing_is_ever_polled() {
    let mut count = 0_u32;
    Offline.poll(&mut |_, _| count += 1);
    Offline.poll(&mut |_, _| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn the_roster_is_empty_and_stays_that_way() {
    assert_eq!(*Offline.peers().get(), PeerSet::new());

    // The empty set is a change to a cursor that has seen nothing -- the same
    // first read every backend owes -- and there is never a second.
    let mut seen = Seen::default();
    assert_eq!(
        Offline.peers().changed_since(&mut seen).as_deref(),
        Some(&PeerSet::new())
    );
    assert!(Offline.peers().changed_since(&mut seen).is_none());
}

#[test]
fn one_watch_is_shared_by_every_offline() {
    // `Offline` is a value, not a handle, so two of them are the same
    // transport. The watch is a process-wide static rather than one per call,
    // which is what lets a caller hold `link.peers()` across calls at all.
    let here = Offline;
    let there = Offline;
    assert!(std::ptr::eq(here.peers(), there.peers()));
}

#[test]
fn it_is_a_transport_behind_a_pointer_like_any_other() {
    // The whole reason for the impl: code written against `&dyn Transport` or
    // `Box<dyn Transport>` takes this without a generic parameter or a second
    // code path. `Transport` requires `Send + Debug`, and `Offline` is both --
    // and its `Debug` prints a name, which `()` could not.
    fn count_peers(link: &dyn Transport) -> usize {
        link.peers().get().len()
    }

    let boxed: Box<dyn Transport> = Box::new(Offline);
    assert_eq!(count_peers(&Offline), 0);
    assert_eq!(count_peers(boxed.as_ref()), 0);
    assert_eq!(format!("{:?}", &Offline as &dyn Transport), "Offline");
    assert_eq!(Offline.datagram_limit(), corvid_net::DATAGRAM_LIMIT);
}

#[test]
fn offline_is_a_type_of_its_own_and_not_a_name_for_unit() {
    // `()` is the type of every statement-shaped call, so an impl on it would
    // let `run(net.all(schedule))` stand in for `run(net.endpoint(seat))` and
    // run offline in silence. What is assertable from here is the positive
    // half, that `Offline` is a transport; that `()` is not one is a
    // `compile_fail` doctest on `Offline`, since a test cannot assert what
    // does not compile.
    fn takes_transport<T: Transport>(link: &T) -> usize {
        link.peers().get().len()
    }

    assert_eq!(takes_transport(&Offline), 0);
}
