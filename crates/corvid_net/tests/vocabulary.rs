//! The value types the contract is written in, checked without a backend.
//!
//! What a backend does with them is that backend's own tests. What is here is
//! the part that has to hold before any backend exists.

use corvid_net::{Channel, Delivery, Lost, PeerId, PeerSet, SendError};

#[test]
fn a_peer_set_is_sorted_whatever_order_it_was_built_in() {
    let mut set = PeerSet::new();
    for peer in [9_u16, 2, 40, 2, 0, 13] {
        set.insert(PeerId(peer));
    }
    assert_eq!(
        set.as_slice(),
        [PeerId(0), PeerId(2), PeerId(9), PeerId(13), PeerId(40)]
    );

    assert!(set.remove(PeerId(9)));
    assert!(!set.remove(PeerId(9)));
    set.insert(PeerId(1));
    assert_eq!(
        set.as_slice(),
        [PeerId(0), PeerId(1), PeerId(2), PeerId(13), PeerId(40)]
    );

    // And it is a value: the same members compare and hash the same however
    // they were put in.
    let backwards: PeerSet = [40, 13, 2, 1, 0].map(PeerId).into_iter().rev().collect();
    assert_eq!(set, backwards);
}

#[test]
fn the_order_survives_every_way_out_of_the_set() {
    // Three ways out, and the sorted order is the reason the type exists -- so
    // each of them is a place the invariant could be handed back broken. The
    // `Vec` conversion is the one with nothing else watching it: it moves the
    // field out wholesale, which is correct only while the field is sorted.
    let set: PeerSet = [40, 2, 13, 2, 0].map(PeerId).into_iter().collect();
    let expected = [PeerId(0), PeerId(2), PeerId(13), PeerId(40)];

    assert_eq!(set.iter().collect::<Vec<_>>(), expected);
    assert_eq!((&set).into_iter().collect::<Vec<_>>(), expected);
    assert_eq!(set.len(), 4);
    assert!(!set.is_empty());
    assert!(PeerSet::new().is_empty());
    assert_eq!(Vec::from(set), expected);
}

#[test]
fn membership_answers_for_a_peer_that_is_there_and_one_that_is_not() {
    // `contains` is the question a backend asks before it routes anything, so
    // one that always answered `false` would be a transport that reaches
    // nobody and one that always answered `true` a transport that claims to
    // reach everybody.
    let set: PeerSet = [40, 2, 13].map(PeerId).into_iter().collect();

    for peer in [PeerId(2), PeerId(13), PeerId(40)] {
        assert!(set.contains(peer), "{peer} is in the set");
    }
    // Below, between, above, and the niche.
    for peer in [PeerId(0), PeerId(3), PeerId(41), PeerId::NONE] {
        assert!(!set.contains(peer), "{peer} is not in the set");
    }
    assert!(!PeerSet::new().contains(PeerId(0)));
}

#[test]
fn nobody_is_nought_and_that_is_what_default_gives() {
    // The niche is at nought rather than at the top of the range, which is the
    // whole reason `PeerId::default()` is safe to have: a default-constructed
    // identifier is an absent peer instead of seat one. Seats are numbered
    // from one so that nought stays free for this.
    assert_eq!(PeerId::NONE, PeerId(0));
    assert_eq!(PeerId::default(), PeerId::NONE);
    assert!(PeerId::NONE.is_none());
    assert!(!PeerId(1).is_none());
    assert!(!PeerId(u16::MAX).is_none());
    assert_eq!(u16::from(PeerId::NONE), 0);

    // A niche in the number rather than a separate type, so it sorts and
    // stores like the rest -- and it sorts *first*, ahead of every real seat.
    assert_eq!(PeerId::from(3), PeerId(3));
    assert!(PeerId::NONE < PeerId(1));
    assert!(PeerId(1) < PeerId(u16::MAX));

    // The display names the type, so a number in a log says which kind it is.
    assert_eq!(PeerId(3).to_string(), "PeerId(3)");
    assert_eq!(PeerId::NONE.to_string(), "PeerId(0)");
}

#[test]
fn every_channel_names_itself_and_the_names_are_these() {
    // Written out rather than derived from `name()`, which would hold for any
    // strings at all. A channel's name goes into a report a person reads, so
    // renaming one is a change to make on purpose.
    //
    // Whether `ALL` still holds every variant needs no test: `named_enum!`
    // generates it from the same list the variants come from, so the two
    // cannot disagree.
    let named: Vec<&str> = Channel::ALL.iter().map(|it| it.name()).collect();
    assert_eq!(named, ["opening", "transfer", "control", "chat"]);

    for &channel in Channel::ALL {
        assert_eq!(channel.to_string(), channel.name());
    }
}

#[test]
fn only_the_two_variants_that_carry_bytes_answer_with_them() {
    assert_eq!(Delivery::Datagram(b"tick").bytes(), Some(&b"tick"[..]));
    assert_eq!(
        Delivery::Stream {
            channel: Channel::Chat,
            bytes: b"good luck",
        }
        .bytes(),
        Some(&b"good luck"[..])
    );
    assert_eq!(Delivery::Joined.bytes(), None);
    assert_eq!(
        Delivery::Lost {
            because: Lost::Reset
        }
        .bytes(),
        None
    );
}

#[test]
fn a_refusal_says_which_numbers_it_refused() {
    // The point of the two-number variants: a caller that has to split a
    // payload or wait out a queue needs the limit as much as the refusal, and
    // reads it off the error rather than off a constant it hard-coded.
    assert_eq!(
        SendError::Unknown(PeerId::NONE).to_string(),
        "no route to PeerId(0)"
    );
    assert_eq!(
        SendError::TooLarge {
            bytes: 1_400,
            limit: 1_200,
        }
        .to_string(),
        "a datagram of 1400 bytes past the limit of 1200"
    );
    assert_eq!(
        SendError::Backpressure {
            waiting: 256,
            limit: 256,
        }
        .to_string(),
        "256 frames are already waiting to be acknowledged and the limit is 256"
    );
    assert_eq!(SendError::Closed.to_string(), "the transport is closed");
}

#[test]
fn every_reason_a_peer_went_away_names_itself_and_the_names_are_these() {
    // Spelled out for the reason the channel names are: this is what a
    // disconnect notice says to a person, and `Display` forwarding to `name`
    // makes any assertion between the two hold whatever the strings become.
    let reasons = Lost::ALL;
    let named: Vec<&str> = reasons.iter().map(|it| it.name()).collect();
    assert_eq!(named, ["closed", "timed out", "refused", "reset"]);

    for &because in reasons {
        assert_eq!(because.to_string(), because.name());
    }
}
