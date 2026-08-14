//! What the runtime does with what a tick asked for, including the ones it
//! cannot do.
//!
//! The rule this file is about is that nothing a tick asks for is dropped
//! silently. Four requests are acted on and every other one is recorded, warned
//! about, and survived -- so the tests below are as much about the requests that
//! go unhandled as about the ones that do not.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::traced::{one_warning_at_a_time, traced};

use common::{APPLAUSE, Counting, FAREWELL, Rules, Scratchpad, Tally, opening};
use corvid_app::Command;
use corvid_app::{Answer, App};
use corvid_behavior::{ExitCode, Scope};
use corvid_time::{Tick, Ticks};

/// How far the runs below play when nothing stops them earlier.
const TICKS: u64 = 12;

/// A run of the honest game with the rules given.
///
/// Its files go in a directory of their own, removed when the run is over. The
/// default is the player's own data directory, which is right for a game and
/// wrong for a test: two tests in one binary would write the same slot, and the
/// run that asked whether a slot was empty would find whatever another test had
/// left there. `tests/saves.rs` is where the directory itself is the subject.
fn play(rules: Rules) -> corvid_app::Outcome<Counting> {
    let scratchpad = Scratchpad::new("commands");
    App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(rules))
        .state(scratchpad.path())
        .for_ticks(Ticks(TICKS))
        .run()
        .unwrap()
}

#[test]
fn quit_stops_the_loop_at_the_tick_that_asked() {
    // The tick at five asks to quit. That tick *ran* -- it is the tick that
    // produced the request -- so the state at six exists and nothing after it
    // does.
    let run = play(Rules {
        quit_at: Some(Tick(5)),
        ..Rules::quiet()
    });

    assert_eq!(run.exit, FAREWELL);
    assert_eq!(run.state.now, Tick(6));
    assert_eq!(run.session.last(), Tick(6));
    // Six rows, at ticks zero through five, and six marks after the opening's.
    assert_eq!(run.session.log.ticks(), 6);
    assert_eq!(run.session.marks.len(), 7);
    assert!(
        run.session
            .log
            .get(Tick(6), corvid_behavior::PlayerId(0))
            .is_none()
    );

    // The request is recorded against the tick that made it and not against the
    // state it produced.
    let quits: Vec<&corvid_app::Request> = run
        .requests
        .iter()
        .filter(|request| matches!(request.command, Command::Quit(_)))
        .collect();
    assert_eq!(quits.len(), 1);
    assert_eq!(quits[0].tick, Tick(5));
    assert_eq!(quits[0].answer, Answer::Done);
    assert_eq!(quits[0].scope, Scope::Global);
}

#[test]
fn the_tick_that_quits_is_the_only_thing_that_moves_the_boundary() {
    // The neighbour, which is what says the test above is about tick five
    // rather than about the number six being right for some other reason. One
    // tick later in the rules is one tick later everywhere.
    let earlier = play(Rules {
        quit_at: Some(Tick(5)),
        ..Rules::quiet()
    });
    let later = play(Rules {
        quit_at: Some(Tick(6)),
        ..Rules::quiet()
    });

    assert_eq!(earlier.session.last(), Tick(6));
    assert_eq!(later.session.last(), Tick(7));
    assert_eq!(later.session.log.ticks(), earlier.session.log.ticks() + 1);

    // And the states the two stopped at are different states, so this is not
    // two runs of a game that had already finished counting.
    assert_ne!(earlier.state, later.state);
}

#[test]
fn a_tick_that_asks_for_two_things_has_both_drained_before_the_loop_stops() {
    // A tick's whole command list is taken, in order, and then the loop breaks.
    // A sink that stopped reading at the `Quit` would lose the screenshot the
    // same tick asked for.
    let run = play(Rules {
        snap_at: Some(Tick(4)),
        quit_at: Some(Tick(4)),
        ..Rules::quiet()
    });

    let at_four: Vec<&Command> = run
        .requests
        .iter()
        .filter(|request| request.tick == Tick(4))
        .map(|request| &request.command)
        .collect();
    assert_eq!(at_four.len(), 2, "{at_four:?}");
    assert_eq!(*at_four[0], Command::Screenshot);
    assert!(matches!(at_four[1], Command::Quit(_)));
    assert_eq!(run.session.last(), Tick(5));
}

#[test]
fn a_save_is_kept_and_can_be_read_back_in_the_same_run() {
    let run = play(Rules {
        save_at: Some(Tick(3)),
        read_at: Some(Tick(7)),
        ..Rules::quiet()
    });

    // A save carries a slot and nothing else: what it writes is the session and
    // the state, both of which the runtime holds, so there are no game bytes to
    // assert on. What is assertable is that the request was made and acted
    // on, which the answers below say.

    let answers: Vec<(Tick, Answer)> = run
        .requests
        .iter()
        .map(|request| (request.tick, request.answer))
        .collect();
    assert_eq!(answers, [(Tick(3), Answer::Done), (Tick(7), Answer::Done)],);
}

#[test]
fn a_read_of_a_slot_nothing_wrote_is_empty_rather_than_handled() {
    // The runtime ran its code and found nothing, which is a different finding
    // from having no code to run. A `Read` answered `Done` here would tell a
    // caller a save had been delivered when none exists.
    let run = play(Rules {
        read_at: Some(Tick(2)),
        ..Rules::quiet()
    });

    assert_eq!(run.requests.len(), 1);
    let request = run.requests.iter().next().unwrap();
    assert_eq!(request.answer, Answer::Empty);
    assert_eq!(request.tick, Tick(2));
    assert_eq!(run.requests.unhandled().count(), 0);
}

#[test]
fn an_unhandled_request_is_recorded_and_warned_about_rather_than_dropped() {
    let log = traced(|| {
        let run = play(Rules {
            cheer_at: Some(Tick(9)),
            ..Rules::quiet()
        });

        // Recorded, with the tick that asked, the scope the vocabulary gives it
        // and the request itself -- so a caller can act on what this runtime
        // could not.
        let unhandled: Vec<&corvid_app::Request> = run.requests.unhandled().collect();
        assert_eq!(unhandled.len(), 1, "{unhandled:?}");
        assert_eq!(unhandled[0].tick, Tick(9));
        assert_eq!(unhandled[0].command, Command::Achieve(APPLAUSE));
        assert_eq!(unhandled[0].scope, Scope::Local);

        // And survived. The run carried on to the tick it was going to stop at.
        assert_eq!(run.session.last(), Tick(TICKS));
        assert_eq!(run.exit, ExitCode::SUCCESS);
    });

    // Warned about, at a level somebody will see (the subscriber below
    // collects nothing quieter), naming the tick and the request. A silent drop is the failure this crate is not allowed to have,
    // and a record nobody is told about is most of the way to one.
    let events = log.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].name, "corvid_app.unhandled");
    assert_eq!(events[0].level, "WARN");
    assert_eq!(events[0].field("tick"), Some("9"));
    assert_eq!(events[0].field("scope"), Some("Local"));
    assert_eq!(
        events[0].field("command"),
        Some("Achieve(AchievementId(1))")
    );
}

#[test]
fn a_handled_request_leaves_no_warning() {
    // The other half of the test above: the warning is about the gap rather
    // than about requests in general, so a run that only asks for things the
    // runtime handles is a quiet run.
    let log = traced(|| {
        let run = play(Rules {
            save_at: Some(Tick(1)),
            read_at: Some(Tick(2)),
            snap_at: Some(Tick(3)),
            quit_at: Some(Tick(4)),
            ..Rules::quiet()
        });
        assert_eq!(run.requests.len(), 4);
        assert_eq!(run.requests.unhandled().count(), 0);
    });

    assert!(log.events().is_empty(), "{:?}", log.events());
}

#[test]
fn every_request_carries_the_scope_the_vocabulary_gives_it() {
    // The scope is recorded rather than recomputed, so it is worth checking
    // that what was recorded is what `Command::scope` says. A runtime that
    // routed on its own classification would be a runtime that disagreed with
    // every peer.
    // This run asks for an achievement, which is a warning -- so it is one of
    // the two tests that touch the callsite `traced` collects from, and it
    // takes the same lock. See [`WARNINGS`].
    let held = one_warning_at_a_time();
    let run = play(Rules {
        save_at: Some(Tick(1)),
        read_at: Some(Tick(2)),
        cheer_at: Some(Tick(3)),
        snap_at: Some(Tick(4)),
        quit_at: Some(Tick(5)),
        ..Rules::quiet()
    });
    drop(held);

    assert_eq!(run.requests.len(), 5);
    for request in &run.requests {
        assert_eq!(request.scope, request.command.scope(), "{request:?}");
    }

    // Both kinds appear, so the loop above is not five copies of one answer.
    let global = run
        .requests
        .iter()
        .filter(|request| request.scope == Scope::Global)
        .count();
    assert_eq!((global, run.requests.len() - global), (3, 2));
}
