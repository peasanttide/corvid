//! What a run with bots in it records.
//!
//! Every assertion here is about a *column of the action log*, because that is
//! what a bot produces and it is the only thing about a bot a session keeps. The
//! game is [`Botted`], whose controller is `()` and whose bot answers
//! [`Action::Bump`] for every seat it is handed: a column of bumps is a seat a
//! bot played, and a column of idles is a seat nothing did.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::{Action, Botted, Rules, Tally, opening, seat};
use corvid_app::App;
use corvid_behavior::PlayerId;
use corvid_replay::Opening;
use corvid_time::Tick;

/// How far the runs below play.
const TICKS: u64 = 10;

/// An opening with a seat for this client and a seat for a bot.
///
/// The fixture's own opening seats one profile, which is a game nobody can be
/// botted into: there would be no seat left over. The second is pushed on here
/// rather than in `common`, because two seats is what *these* tests need and the
/// rest of them are written against one.
fn two_seats() -> Opening<Tally> {
    let mut opening = opening::<Tally>(Rules::quiet());
    opening.roster.push(seat(1001));
    opening
}

/// The action recorded for `player` on the first tick of `outcome`'s session.
fn first(outcome: &corvid_app::Outcome<Botted>, player: PlayerId) -> Action {
    outcome
        .session
        .log
        .get(Tick::ZERO, player)
        .copied()
        .expect("the first tick is in the log")
}

#[test]
fn a_bot_takes_the_seat_this_client_is_not_playing() {
    let outcome = App::<Botted>::new()
        .headless()
        .opening(two_seats())
        .seat(PlayerId(0))
        .bots(1)
        .for_ticks(TICKS)
        .run()
        .expect("a run with one bot");

    // Seat zero is this client's, and its controller is `()`. Seat one is the
    // one the bot was given, in roster order, because seat zero was taken.
    assert_eq!(first(&outcome, PlayerId(0)), Action::Idle);
    assert_eq!(first(&outcome, PlayerId(1)), Action::Bump);
}

/// The other half of the claim above, which that test cannot make on its own: a
/// seat is idle unless a bot is asked for, so the bump came from the bot rather
/// than from anything the runtime does anyway.
#[test]
fn a_run_with_no_bots_leaves_the_other_seat_idle() {
    let outcome = App::<Botted>::new()
        .headless()
        .opening(two_seats())
        .seat(PlayerId(0))
        .for_ticks(TICKS)
        .run()
        .expect("a run with no bots");

    assert_eq!(first(&outcome, PlayerId(1)), Action::Idle);
    assert_eq!(outcome.state.count, 0, "something moved the tally");
}

/// A bot answers every tick rather than only the first, which is the difference
/// between a seat that was filled and a seat that was initialised.
#[test]
fn a_bot_answers_for_every_tick_it_played() {
    let outcome = App::<Botted>::new()
        .headless()
        .opening(two_seats())
        .seat(PlayerId(0))
        .bots(1)
        .for_ticks(TICKS)
        .run()
        .expect("a run with one bot");

    for tick in 0..TICKS {
        assert_eq!(
            outcome.session.log.get(Tick(tick), PlayerId(1)).copied(),
            Some(Action::Bump),
            "tick {tick}",
        );
    }
    // And the simulation saw them: one bump a tick, at the quiet rules' step.
    assert_eq!(
        outcome.state.count,
        i64::try_from(TICKS).expect("ten fits") * Rules::quiet().step,
    );
}

/// A spectator plays nobody, so there is no seat for a bot to skip and both
/// columns are a bot's.
#[test]
fn a_spectator_lets_bots_take_every_seat() {
    let outcome = App::<Botted>::new()
        .headless()
        .opening(two_seats())
        .spectating()
        .bots(2)
        .for_ticks(TICKS)
        .run()
        .expect("a run with two bots");

    assert_eq!(first(&outcome, PlayerId(0)), Action::Bump);
    assert_eq!(first(&outcome, PlayerId(1)), Action::Bump);
}

/// The number a caller asked for and the number the roster has are two separate
/// facts, and the roster is the one that is true.
#[test]
fn more_bots_than_seats_fills_the_seats_there_are() {
    let outcome = App::<Botted>::new()
        .headless()
        .opening(two_seats())
        .spectating()
        .bots(u16::MAX)
        .for_ticks(1)
        .run()
        .expect("a run asked for more bots than seats");

    assert_eq!(outcome.session.opening.roster.len(), 2);
    assert_eq!(first(&outcome, PlayerId(0)), Action::Bump);
    assert_eq!(first(&outcome, PlayerId(1)), Action::Bump);
}

/// Bots on the linked path are refused rather than reconciled: a controller is
/// no part of what a session records, so two peers each filling the same seat
/// locally would be two answers with nothing to choose between them.
#[cfg(feature = "net")]
mod linked {
    use super::{TICKS, two_seats};
    use corvid_app::App;
    use corvid_net::{Channel, Delivery, PeerId, PeerSet, SendError, Transport};
    use corvid_signal::{Watch, channel as watch};

    /// A transport that carries nothing, which is all a refusal needs: the run
    /// is turned away before anything is opened, sent or polled.
    #[derive(Debug)]
    struct Unused {
        /// Nobody, published once.
        peers: Watch<PeerSet>,
    }

    impl Unused {
        /// One reaching no peers.
        fn new() -> Self {
            let (emitter, peers) = watch("peers", PeerSet::default());
            drop(emitter);
            Self { peers }
        }
    }

    impl Transport for Unused {
        fn send_datagram(&self, _to: PeerId, _bytes: &[u8]) -> Result<(), SendError> {
            Ok(())
        }

        fn send_stream(
            &self,
            _to: PeerId,
            _channel: Channel,
            _bytes: &[u8],
        ) -> Result<(), SendError> {
            Ok(())
        }

        fn poll(&self, _sink: &mut dyn FnMut(PeerId, Delivery<'_>)) {}

        fn peers(&self) -> &Watch<PeerSet> {
            &self.peers
        }
    }

    #[test]
    fn bots_and_a_transport_are_refused() {
        let why = App::<super::Botted>::new()
            .headless()
            .opening(two_seats())
            .transport(Box::new(Unused::new()))
            .bots(1)
            .for_ticks(TICKS)
            .run()
            .expect_err("bots and a transport");

        assert!(
            matches!(why, corvid_app::Error::BotsAndPeers { bots: 1 }),
            "{why:?}",
        );
    }

    /// And a transport on its own is not refused, so the check above is about
    /// the pair rather than about the transport.
    #[test]
    fn a_transport_with_no_bots_is_not_refused() {
        let outcome = App::<super::Botted>::new()
            .headless()
            .opening(two_seats())
            .transport(Box::new(Unused::new()))
            .for_ticks(0)
            .run()
            .expect("a linked run of no ticks");

        assert_eq!(outcome.session.opening.roster.len(), 2);
    }
}
