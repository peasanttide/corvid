//! Two peers, three links, and the claims that make this a multiplayer example
//! rather than a picture of one.
//!
//! Everything here drives [`pong::rally::Match`], the netcode lab: two peers,
//! one link that loses and delays what a seed tells it to, and no clock. There
//! is one implementation of "two peers playing below the runtime" and this is it
//! with assertions on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]

use corvid::PlayerId;

use corvid::digest;

use corvid_lockstep::{Budget, Datagram, Halt};

use corvid::Tick;
use corvid_net::{Lost, PeerId};
use corvid_net_mock::Schedule;
use pong::{
    Move,
    rally::{Match, Policy, Trace, agreed},
};

/// How long a session in this file plays. Nine hundred ticks is thirty seconds
/// at this game's rate, which is long enough for a mobile link to lose several
/// hundred packets and for the peers to have corrected each one.
const TICKS: u64 = 900;

/// The seed every link in this file lies with.
const SEED: u64 = 0x0f_1e_2d_3c;

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// Every tick both peers have every seat's real action for, compared digest by
/// digest.
///
/// This is the assertion the whole example exists to support, and it is worth
/// being exact about what it does *not* say: it says nothing about the ticks
/// above the confirmed line, where each peer has predicted the other and the
/// two are expected to disagree until a datagram settles it.
fn traces_agree(traces: &[Trace]) -> Result<Tick, String> {
    let line = agreed(traces);
    let Some((first, rest)) = traces.split_first() else {
        return Err("a match with no peers in it".to_owned());
    };
    for other in rest {
        for at in 0..=line.0 {
            let here = first.mark(Tick(at));
            let there = other.mark(Tick(at));
            if here != there {
                return Err(format!(
                    "seat {} and seat {} disagree at tick {at}: {here:?} against {there:?}, \
                     with every seat confirmed through {}",
                    first.seat.0, other.seat.0, line.0,
                ));
            }
        }
    }
    Ok(line)
}

/// A perfect link confirms every action before the tick it belongs to, so
/// nothing is ever predicted wrongly and nothing rolls back.
#[test]
fn a_perfect_link_never_rolls_back() -> Fallible {
    let mut playing = Match::new(Schedule::PERFECT, SEED, [Policy::Chase, Policy::Chase])?;
    playing.play(TICKS)?;

    let line = traces_agree(playing.traces())?;
    assert!(
        line.0 >= TICKS - 8,
        "a perfect link should confirm almost to the end, and it confirmed to {}",
        line.0,
    );
    for trace in playing.traces() {
        assert_eq!(
            trace.rollbacks, 0,
            "seat {} rolled back on a link that loses and delays nothing",
            trace.seat.0,
        );
    }
    Ok(())
}

/// A domestic link loses a few and delays a few. The peers correct each one and
/// agree about every tick they have both heard about.
#[test]
fn a_domestic_link_agrees() -> Fallible {
    let mut playing = Match::new(Schedule::DOMESTIC, SEED, [Policy::Chase, Policy::Chase])?;
    playing.play(TICKS)?;

    let line = traces_agree(playing.traces())?;
    // Both peers open on the same `opening()`, so tick zero agrees before
    // anything has happened. Without a floor on how far the confirmed line got,
    // a regression that stalled this schedule near the start would pass here by
    // comparing one trivially identical mark — which is the failure the mobile
    // test guards against with its rollback count and this one had nothing
    // against at all.
    assert!(
        line.0 >= TICKS - 8,
        "a domestic link should confirm almost to the end, and it confirmed to {}",
        line.0,
    );
    Ok(())
}

/// **The deliverable.** A mobile link mispredicts constantly, the peers roll
/// back constantly, and their digests are identical over every confirmed tick.
///
/// The rollback counts are asserted to be non-zero, because a convergence test
/// that passed because nothing was ever predicted wrongly would be testing
/// nothing at all — and the depth is asserted to stay inside the budget,
/// because a peer that re-simulated more than that in one tick would be a peer
/// that missed its frame.
#[test]
fn a_mobile_link_rolls_back_and_still_agrees() -> Fallible {
    let mut playing = Match::new(Schedule::MOBILE, SEED, [Policy::Chase, Policy::Chase])?;
    playing.play(TICKS)?;

    traces_agree(playing.traces())?;
    for trace in playing.traces() {
        assert!(
            trace.rollbacks > 10,
            "seat {} rolled back {} times on a mobile link, which is too few for this test \
             to be measuring prediction at all",
            trace.seat.0,
            trace.rollbacks,
        );
        assert!(
            trace.deepest > 0 && trace.deepest <= Budget::DEFAULT.rollback,
            "seat {} rolled back {} ticks at once, and the budget is {}",
            trace.seat.0,
            trace.deepest,
            Budget::DEFAULT.rollback,
        );
    }
    Ok(())
}

/// The same session, twice, is the same session.
///
/// A `MockNet` draws its latency, jitter, loss and reorder from a hash of the
/// seed rather than from a clock, and the game has no randomness at all — so a
/// failure in any test here can be reproduced rather than chased.
#[test]
fn a_session_is_reproducible() -> Fallible {
    let mut once = Match::new(Schedule::MOBILE, SEED, [Policy::Chase, Policy::Chase])?;
    once.play(400)?;
    let mut twice = Match::new(Schedule::MOBILE, SEED, [Policy::Chase, Policy::Chase])?;
    twice.play(400)?;
    assert_eq!(once.traces(), twice.traces());

    let mut other = Match::new(Schedule::MOBILE, SEED + 1, [Policy::Chase, Policy::Chase])?;
    other.play(400)?;
    assert_ne!(
        once.traces(),
        other.traces(),
        "two different seeds produced the same link, which would make the seed decorative",
    );
    Ok(())
}

/// A total outage stalls both peers and desyncs neither, and restoring the link
/// resumes the session where it left off.
///
/// This is the case a lockstep design is most often accused of getting wrong.
/// A peer that hears nothing predicts as far as its budget allows and then
/// waits; it does not carry on into a future nobody has agreed to, and it does
/// not decide the other player has left.
#[test]
fn a_total_outage_is_not_a_desync() -> Fallible {
    let mut playing = Match::new(Schedule::PERFECT, SEED, [Policy::Chase, Policy::Chase])?;
    playing.play(120)?;
    let before = agreed(playing.traces());

    // Everything lost, both ways.
    playing.net().all(Schedule {
        loss: corvid::Factor16::ONE,
        ..Schedule::PERFECT
    });
    playing.play(120)?;
    let during = agreed(playing.traces());
    for trace in playing.traces() {
        assert!(
            trace.stalls > 0,
            "seat {} played through a total outage without ever stalling, which means it \
             predicted a decision nobody made",
            trace.seat.0,
        );
    }
    assert!(
        during.0 <= before.0 + u64::from(Budget::DEFAULT.ahead) + 2,
        "the confirmed line moved from {} to {} while nothing was arriving",
        before.0,
        during.0,
    );

    // And back.
    playing.net().all(Schedule::PERFECT);
    playing.play(240)?;
    let after = traces_agree(playing.traces())?;
    assert!(
        after.0 > during.0 + 100,
        "the session did not resume: confirmed through {} during the outage and {} after it",
        during.0,
        after.0,
    );
    Ok(())
}

/// A peer that goes away entirely is predicted rather than reported, and the
/// session carries on.
#[test]
fn a_cut_link_stalls_rather_than_halting() -> Fallible {
    let mut playing = Match::new(Schedule::PERFECT, SEED, [Policy::Chase, Policy::Idle])?;
    playing.play(90)?;
    playing.net().cut(PeerId(0), PeerId(1), Lost::TimedOut);
    // No error, which is the assertion: a peer with nobody to talk to is a peer
    // that predicts and waits.
    playing.play(90)?;
    Ok(())
}

/// A digest that disagrees stops the session and says where.
///
/// The datagram below is built by hand with a mark that is not this session's,
/// which is what a peer whose `tick` is not a pure function of its arguments
/// would send. Nothing else in this workspace can produce one — that is the
/// point of the rest of the design — so the check is exercised by forging it.
#[test]
fn a_disagreeing_digest_halts_the_session() -> Fallible {
    use corvid_lockstep::Peer;
    use corvid_replay::Session;

    let mut here =
        Peer::<pong::Table>::new(Session::new(pong::opening())?, PlayerId(0), Budget::DEFAULT);
    let mut there =
        Peer::<pong::Table>::new(Session::new(pong::opening())?, PlayerId(1), Budget::DEFAULT);

    // Ten honest ticks, so that both peers have confirmed a stretch of the
    // session and have marks to compare.
    for _ in 0..10 {
        here.submit(Move::Up)?;
        there.submit(Move::Down)?;
        let (mine, yours) = (here.outgoing(), there.outgoing());
        here.receive(&yours)?;
        there.receive(&mine)?;
        here.advance(&mut corvid::Discard::new())?;
        there.advance(&mut corvid::Discard::new())?;
    }
    assert_eq!(digest(here.state()), digest(there.state()));

    // And one dishonest one: the same actions, a mark that is not the state's.
    let mut lie = there.outgoing();
    lie.mark = corvid::Digest::from_u64(0xdead_beef);
    let refused = here.receive(&lie);
    match refused {
        Err(Halt::Desync(desync)) => {
            assert_eq!(desync.peer, PlayerId(1));
            assert!(
                desync.at <= here.tick(),
                "a desync was reported at tick {} and this peer has only reached {}",
                desync.at.0,
                here.tick().0,
            );
        }
        other => panic!("a forged digest was not caught: {other:?}"),
    }
    Ok(())
}

/// A datagram naming a tick far past the horizon is ignored, and the session
/// carries on.
///
/// Two properties in one, and the second is the one that took a bug to find.
/// The log does not grow to meet it — a tick is the one number in a session
/// that arrives from somewhere else, and growing to whatever it said would be a
/// request for as much memory as the number. And the peer does *not* stop: a
/// session anybody with a socket could end by sending one large number would be
/// a worse failure than the memory it was protecting.
#[test]
fn a_tick_past_the_horizon_is_ignored() -> Fallible {
    use corvid_lockstep::Peer;
    use corvid_replay::Session;

    let mut here =
        Peer::<pong::Table>::new(Session::new(pong::opening())?, PlayerId(0), Budget::DEFAULT);
    let hostile: Datagram<Move> = Datagram {
        seat: PlayerId(1),
        first: Tick(u64::MAX / 2),
        actions: vec![Move::Up; corvid_lockstep::WINDOW],
        heard: None,
        // Marked at the same impossible tick. A mark for a tick this peer has
        // never reached says nothing about it — which is what is being tested
        // here; a mark for tick zero would be caught by the digest check
        // instead, and that check has a test of its own above.
        marked: Tick(u64::MAX / 2),
        mark: corvid::Digest::ZERO,
    };
    let rolled = here.receive(&hostile)?;
    assert!(!rolled.happened(), "a hostile datagram caused a rollback");
    assert_eq!(here.session.log.first(), Tick::ZERO);
    assert!(
        here.session.log.ticks() < 100,
        "the log grew to {} rows for a datagram naming tick {}",
        here.session.log.ticks(),
        u64::MAX / 2,
    );

    // And the session is still playable afterwards.
    here.submit(Move::Up)?;
    here.advance(&mut corvid::Discard::new())?;
    assert_eq!(here.tick(), Tick(1));
    Ok(())
}
