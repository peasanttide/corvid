//! Two peers in one process, and the thread the second one runs on.
//!
//! The seam against `mod.rs` is the process: a [`Match`](crate::rally::Match) is one
//! machine's half of a session and has no thread in it, and this is what puts
//! two of them beside each other so the demo and the tests are one piece of
//! code.

use std::time::Duration;

use corvid::{Controller as _, PlayerId, Session, Tick, Ticks};
use corvid_lockstep::{Budget, Datagram, Peer};
use corvid_net::{Delivery, Transport};
use corvid_net_mock::{MockNet, Schedule};

use crate::rally::{Policy, Rallying, SEED, Trace, peer_at, seats};
use crate::{Move, SEATS, Table, opening};

/// Plays both seats in this process: one in a window, one on a thread.
///
/// **This is real netcode against a real peer**, and the only thing about it
/// that is not two machines is that the datagrams never leave the address
/// space. The opponent below is a whole [`Peer`] -- it predicts this player's
/// paddle, rolls back when the prediction is wrong, and sends its digest every
/// tick -- sitting behind a [`MockNet`] on a domestic curve, so what the player
/// is playing against is a session with latency and loss in it rather than a
/// second paddle in the same simulation.
///
/// What it is *not* is an interesting opponent: it chases the ball, which is
/// [`Policy::Chase`] and is four lines. The netcode is the exhibit.
///
/// The run is handed back rather than swallowed, so that a test can read what
/// the netcode did -- which is the only way to tell this mode from a single-seat
/// run with a picture of an opponent in it.
///
/// # Errors
///
/// Whatever the run answers, and [`Error::Shape`](corvid::Error::Shape) if
/// the opening cannot be made into a session.
#[cfg(feature = "window")]
pub fn together(
    seat: PlayerId,
    ticks: Option<u64>,
    windowed: bool,
) -> corvid::Result<corvid::Outcome<Rallying>> {
    use corvid::{App, Error, Game, Input};
    let net = MockNet::new(seats(), SEED);
    net.all(Schedule::DOMESTIC);

    let other = PlayerId(u16::from(seat.0 == 0));
    let opponent = net.endpoint(corvid::peer_of(other));
    let session = Session::new(opening()).map_err(Error::Shape)?;
    let clock = net.clone();
    let period = Rallying::PERIOD.period();
    // Detached on purpose: the window owns the process, and when it closes the
    // process ends and this goes with it. A join handle would be a promise to
    // wait for a loop with no way out of it.
    drop(std::thread::spawn(move || {
        opponent_loop(session, other, &opponent, &clock, period);
    }));

    let app = App::<Rallying>::new()
        .opening(opening())
        .seat(seat)
        .transport(Box::new(net.endpoint(corvid::peer_of(seat))))
        .input(Input::new(crate::action::SETS))
        .bindings(crate::action::bindings());
    // A window is the point of this mode and not a requirement of it: without
    // one it is the same two peers with nobody watching, which is what makes it
    // something a build machine can run.
    let app = if windowed { app.window() } else { app };
    let app = match ticks {
        Some(ticks) => app.for_ticks(Ticks(ticks)),
        None => app,
    };
    app.run()
}

/// The opponent: one peer, one policy, and a link whose clock this drives.
///
/// It sleeps to the tick rather than spinning, and it advances the mock link's
/// clock by one period per tick -- so the latency a `MockNet` schedule describes
/// passes at the same rate the game does, and the player in the window is
/// playing over a link that behaves like the one [`Match`](crate::rally::Match) measures.
#[cfg(feature = "window")]
fn opponent_loop(
    session: Session<Table>,
    seat: PlayerId,
    endpoint: &corvid_net_mock::Endpoint,
    net: &MockNet,
    period: Duration,
) {
    let mut peer = Peer::new(session, seat, Budget::DEFAULT);
    let mut due = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        if now < due {
            std::thread::sleep(due - now);
            continue;
        }
        due += period;

        let action = Policy::Chase.action(corvid::Acting {
            state: peer.state(),
            input: &corvid::Input::new(crate::action::SETS),
            time: corvid::Time {
                tick: peer.tick(),
                ..corvid::Time::default()
            },
            seat,
        });
        if peer.submit(action).is_err() {
            return;
        }

        let mut arrived: Vec<Vec<u8>> = Vec::new();
        endpoint.poll(&mut |_from, delivery| {
            if let Delivery::Datagram(bytes) = delivery {
                arrived.push(bytes.to_vec());
            }
        });
        for bytes in &arrived {
            let Ok(datagram) = corvid_wire::decode::<Datagram<Move>>(bytes) else {
                continue;
            };
            // A peer that cannot carry on stops playing. The window's own run
            // reports the same condition as an error; this thread has nobody to
            // report to, and carrying on with a halted peer would put a paddle
            // on the screen that is no longer part of the session.
            if peer.receive(&datagram).is_err() {
                return;
            }
        }
        let outgoing = peer.outgoing();
        if peer.advance(&mut corvid::Discard::new()).is_err() {
            return;
        }
        if let Ok(bytes) = corvid_wire::encode(&outgoing) {
            for other in 0..SEATS {
                if other != usize::from(seat.0) {
                    // As above: a peer that cannot be reached is predicted
                    // through rather than reported.
                    let _unreachable = endpoint.send_datagram(peer_at(other), &bytes);
                }
            }
        }
        net.advance(period);
    }
}

/// The newest tick every peer in a [`Match`](crate::rally::Match) has every seat's real action for,
/// which is the range two traces can honestly be compared over.
///
/// It is the minimum of two minimums: over the peers, and within each peer over
/// the seats. A mark above it was taken over a state one peer predicted part
/// of, and two predictions disagreeing is what prediction is.
#[must_use]
pub fn agreed(traces: &[Trace]) -> Tick {
    traces
        .iter()
        .map(|trace| trace.confirmed.min(trace.tick))
        .min()
        .unwrap_or(Tick::ZERO)
}
