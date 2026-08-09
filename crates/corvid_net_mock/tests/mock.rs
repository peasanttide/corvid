//! The determinism underneath every curve, and the frozen digests that pin it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

mod common;

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::{Channel, Lost, PeerId, SendError, Transport};
use corvid_net_mock::{MockNet, Schedule};

use crate::common::{Heard, OTHER, SEED, conversation, drain, numbered};

#[test]
fn the_same_seed_and_script_deliver_the_same_run() {
    for schedule in [Schedule::PERFECT, Schedule::DOMESTIC, Schedule::MOBILE] {
        let once = conversation(schedule, SEED);
        assert_eq!(once, conversation(schedule, SEED));
    }

    // A perfect link draws nothing it can act on, so its run does not depend
    // on the seed. A link that lies does, and the seed is the whole of what it
    // lies from.
    assert_eq!(
        conversation(Schedule::PERFECT, SEED),
        conversation(Schedule::PERFECT, OTHER)
    );
    assert_ne!(
        conversation(Schedule::DOMESTIC, SEED),
        conversation(Schedule::DOMESTIC, OTHER)
    );
    assert_ne!(
        conversation(Schedule::MOBILE, SEED),
        conversation(Schedule::MOBILE, OTHER)
    );
}

#[test]
fn a_cut_is_told_once_and_refuses_everything_after() {
    let net = MockNet::new(2, 1);
    net.all(Schedule::DOMESTIC);
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&net, &bob);
    drain(&net, &alice);

    alice.send_datagram(PeerId(2), b"in flight").unwrap();
    alice
        .send_stream(PeerId(2), Channel::Transfer, b"also in flight")
        .unwrap();
    net.cut(PeerId(1), PeerId(2), Lost::Reset);

    let both: Vec<_> = [
        drain(&net, &alice),
        drain(&net, &bob),
        {
            net.advance(Duration::from_millis(500));
            drain(&net, &alice)
        },
        drain(&net, &bob),
    ]
    .concat();

    // One loss each, and nothing that was in flight when the link went.
    assert_eq!(
        both.iter()
            .map(|(_, _, one)| one.clone())
            .collect::<Vec<_>>(),
        [Heard::Lost(Lost::Reset), Heard::Lost(Lost::Reset)]
    );
    assert_eq!(net.tally().in_flight, 0);

    assert_eq!(
        alice.send_datagram(PeerId(2), b"anyone there"),
        Err(SendError::Unknown(PeerId(2)))
    );
    assert_eq!(
        bob.send_stream(PeerId(1), Channel::Control, b"anyone there"),
        Err(SendError::Unknown(PeerId(1)))
    );

    // Cutting again says nothing, because there is nothing left to sever.
    net.cut(PeerId(1), PeerId(2), Lost::Closed);
    assert_eq!(drain(&net, &alice), []);
    assert_eq!(drain(&net, &bob), []);
}

#[test]
fn a_stream_over_a_half_lost_link_arrives_whole_and_in_order() {
    let net = MockNet::new(2, 0xa11);
    net.all(Schedule::new(
        Duration::from_millis(50),
        Duration::from_millis(10),
        Factor16::from_f64(0.5),
        Factor16::from_f64(0.5),
    ));
    let alice = net.endpoint(PeerId(1));
    let bob = net.endpoint(PeerId(2));
    drain(&net, &bob);

    let state: Vec<Vec<u8>> = (0..32_u32).map(numbered).collect();
    for frame in &state {
        alice
            .send_stream(PeerId(2), Channel::Transfer, frame)
            .unwrap();
    }

    let mut arrived = Vec::new();
    let mut steps = 0;
    while arrived.len() < state.len() && steps < 2_000 {
        net.advance(Duration::from_millis(10));
        steps += 1;
        arrived.extend(
            drain(&net, &bob)
                .into_iter()
                .filter_map(|(_, _, one)| match one {
                    Heard::Stream(Channel::Transfer, bytes) => Some(bytes),
                    _ => None,
                }),
        );
    }

    assert_eq!(arrived, state);
    assert!(net.tally().dropped > 0, "a half-lost link lost nothing");
    assert_eq!(net.tally().in_flight, 0);

    // Head-of-line blocking is what makes a transfer over a bad link take a
    // visible amount of time: thirty-two frames of fifty milliseconds each
    // cannot have arrived in less than that.
    assert!(net.elapsed() >= Duration::from_millis(50 * 32));
}

/// The digests of the three shipped curves, frozen.
///
/// Comparing two runs in one process says the network does not consult
/// anything that varies *within* a process. It cannot see a schedule that
/// moves between builds, targets or versions of a dependency -- both sides of
/// that comparison move together. Two peers on two machines are the case this
/// crate exists for, and a literal nobody regenerated is the only thing that
/// notices when their answers stop matching.
///
/// **Changing a value here is a change to every recorded session.**
///
/// `PERFECT` is the control for a change to the draw stream: it has no loss, no
/// jitter and no reorder, so no draw can reach its outcome, and only the other
/// two move when the generator does. It is not a control for a change to the
/// script, since the peer numbers and payloads are in the trace all three
/// digest over.
#[test]
fn the_three_curves_digest_as_they_were_recorded() {
    assert_eq!(
        conversation(Schedule::PERFECT, SEED).to_u64(),
        0x771f_75d8_ce2d_b98d
    );
    assert_eq!(
        conversation(Schedule::DOMESTIC, SEED).to_u64(),
        0xcde8_a01a_6f9e_028f
    );
    assert_eq!(
        conversation(Schedule::MOBILE, SEED).to_u64(),
        0x77ce_1397_43e2_744b
    );
}
