//! What the runtime does with what a tick asked for, including the ones it
//! cannot do.
//!
//! The rule this file is about is that nothing a tick asks for is dropped
//! silently. Four requests are acted on and every other one is recorded, warned
//! about, and survived — so the tests below are as much about the requests that
//! go unhandled as about the ones that do not.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::{
    fmt, fs,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use common::{
    APPLAUSE, Attending, Counting, FAREWELL, Rules, SLOT, Scratchpad, Tally, attendance, opening,
    seat,
};
use corvid_app::Command;
use corvid_app::{Answer, App};
use corvid_behavior::{ExitCode, Presence, ProfileId, Scope};
use corvid_replay::Profile;
use corvid_time::{Tick, Ticks};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

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
    // The tick at five asks to quit. That tick *ran* — it is the tick that
    // produced the request — so the state at six exists and nothing after it
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
    let quits: Vec<&corvid_app::Request<common::Ref>> = run
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

    let at_four: Vec<&Command<common::Ref>> = run
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

    // A save carries a slot and nothing else. It used to carry the game's own
    // bytes and this asserted on them — but what a save writes is the session
    // and the state, both of which the runtime holds, and nothing ever read the
    // blob back. So what is assertable is that the request was made and acted
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
        // and the request itself — so a caller can act on what this runtime
        // could not.
        let unhandled: Vec<&corvid_app::Request<common::Ref>> = run.requests.unhandled().collect();
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
    // This run asks for an achievement, which is a warning — so it is one of
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

#[test]
fn a_screenshot_is_recorded_and_no_picture_is_written() {
    // Nothing here turns a screenshot into a file, so the request
    // is answered by writing down that it was made, and a capture that grew a
    // `.png` would be this crate claiming something it cannot do.
    let scratchpad = Scratchpad::new("screenshot");
    let run = App::<Counting>::new()
        .headless()
        .capture(scratchpad.path())
        .opening(opening::<Tally>(Rules {
            snap_at: Some(Tick(2)),
            ..Rules::quiet()
        }))
        .for_ticks(Ticks(4))
        .run()
        .unwrap();

    let request = run.requests.iter().next().unwrap();
    assert_eq!(request.command, Command::Screenshot);
    assert_eq!(request.answer, Answer::Done);
    assert_eq!(request.tick, Tick(2));

    let names: Vec<String> = fs::read_dir(scratchpad.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|name| {
            !std::path::Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        }),
        "a capture wrote a picture: {names:?}",
    );
}

#[test]
fn a_run_that_asks_for_nothing_records_nothing() {
    // The floor the tests above are measured from.
    let run = play(Rules::quiet());
    assert!(run.requests.is_empty());
    assert_eq!(run.requests.len(), 0);
    assert_eq!(run.exit, ExitCode::SUCCESS);
}

#[test]
fn the_first_quit_a_tick_asks_for_is_the_one_the_run_exits_with() {
    // A tick may return two `Quit`s — the vocabulary says nothing against it,
    // and a game that quits from two places in one tick writes exactly that.
    // The sink documents that the first wins, and until this fixture existed
    // that sentence was a comment on a branch nothing took: deleting the
    // `if self.quit.is_none()` guard passed the whole workspace.
    let second = ExitCode(9);
    assert_ne!(second, FAREWELL);

    let run = play(Rules {
        quit_at: Some(Tick(3)),
        then_quit_with: Some(second),
        ..Rules::quiet()
    });

    assert_eq!(run.exit, FAREWELL);

    // Both were recorded, at the tick that asked, in the order the tick
    // returned them — a sink that let the second win would have taken the same
    // two requests and answered with the other status.
    let quits: Vec<&Command<common::Ref>> = run
        .requests
        .iter()
        .filter(|request| matches!(request.command, Command::Quit(_)))
        .map(|request| &request.command)
        .collect();
    assert_eq!(quits.len(), 2, "{quits:?}");
    assert_eq!(*quits[0], Command::Quit(FAREWELL));
    assert_eq!(*quits[1], Command::Quit(second));

    // And the boundary is the one a single `Quit` has: the tick that asked ran,
    // and nothing after it did.
    assert_eq!(run.state.now, Tick(4));
    assert_eq!(run.session.last(), Tick(4));

    // The neighbour, which says the status above is the *first* of the two
    // rather than the one this fixture is built with. The same tick, the same
    // two statuses, in the other order: now the run exits with the other one.
    let reversed = play(Rules {
        quit_at: Some(Tick(3)),
        quit_with: Some(second),
        then_quit_with: Some(FAREWELL),
        ..Rules::quiet()
    });
    assert_eq!(reversed.exit, second);
    assert_eq!(reversed.session.last(), run.session.last());
}

#[test]
fn the_roster_the_loop_ticks_with_is_the_one_the_session_records() {
    // The thing every command test above rests on, and the loop is what is
    // being asked rather than `Profile::presence_at`: the fixture writes down
    // the roster its *tick* was handed, so what is asserted here came out of
    // `Runtime::simulate` and not out of a helper called twice.
    //
    // The roster is three profiles that do three different things, because a
    // one-seat roster answers the same way under a loop that filtered nothing,
    // numbered every seat zero, or handed the tick the whole roster whatever
    // the tick was.
    let run = App::<Attending>::new()
        .headless()
        .opening(attendance(vec![
            seat(1000),
            Profile {
                account: ProfileId(1001),
                joined: Tick(2),
                left: None,
            },
            Profile {
                account: ProfileId(1002),
                joined: Tick::ZERO,
                left: Some(Tick(3)),
            },
        ]))
        .for_ticks(Ticks(5))
        .run()
        .unwrap();

    let seats = |at: usize| -> Vec<(u16, Presence)> {
        run.state.rolls[at]
            .seats
            .iter()
            .map(|seen| (seen.id.0, seen.presence))
            .collect()
    };

    // Tick zero: the seat that has not joined yet is *absent from the slice*,
    // and the two that have joined are `Joining` on that tick and only then.
    assert_eq!(
        seats(0),
        [
            (
                0,
                Presence::Joining {
                    profile: ProfileId(1000)
                }
            ),
            (
                2,
                Presence::Joining {
                    profile: ProfileId(1002)
                }
            ),
        ],
    );
    assert_eq!(seats(1), [(0, Presence::Active), (2, Presence::Active)]);

    // Tick two: the late seat arrives, in its own seat — seat one, which is its
    // position in the roster and not the position it holds in this slice.
    assert_eq!(
        seats(2),
        [
            (0, Presence::Active),
            (
                1,
                Presence::Joining {
                    profile: ProfileId(1001)
                }
            ),
            (2, Presence::Active),
        ],
    );

    // Tick three: the seat that left is still in the roster, submitting the
    // default forever, which is what `Dropped` is for. A loop that dropped it
    // from the slice would renumber nothing and lose a seat.
    assert_eq!(
        seats(3),
        [
            (0, Presence::Active),
            (1, Presence::Active),
            (2, Presence::Dropped { since: Tick(3) }),
        ],
    );
    assert_eq!(seats(4), seats(3));

    // Five ticks, and the fixture keeps no counter to say so: the length of the
    // record is the number of ticks that ran.
    assert_eq!(run.state.rolls.len(), 5);
    assert_eq!(run.session.last(), Tick(5));

    // A save request used to carry the game's own bytes, and this asserted
    // nothing here rewrote them. It carries a slot and nothing else now: what a
    // save writes is the session and the state, both of which the runtime
    // holds, and the blob a tick handed over had no route back on reload.
    assert_eq!(SLOT, SLOT);
}

// -- the subscriber -------------------------------------------------------

/// One event, as a subscriber saw it.
#[derive(Clone, Debug, Default)]
struct Recorded {
    /// The callsite's name.
    name: String,
    /// Its level, as `tracing` prints it.
    level: String,
    /// Every field, in the order it arrived.
    fields: Vec<(String, String)>,
}

impl Recorded {
    /// What one field was recorded as.
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(recorded, _)| recorded == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Everything a subscriber collected.
#[derive(Default)]
struct Log {
    /// The warnings, in order. Spans are not collected: this crate opens none.
    events: Mutex<Vec<Recorded>>,
}

impl Log {
    fn events(&self) -> Vec<Recorded> {
        lock(&self.events).clone()
    }
}

/// The lock, with poisoning ignored, so a panic in one test reports as that
/// test failing rather than as every later one failing to read the log.
fn lock<T>(what: &Mutex<T>) -> MutexGuard<'_, T> {
    what.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A subscriber that records rather than prints.
struct Recorder(Arc<Log>);

impl Subscriber for Recorder {
    /// Warnings and above, and nothing else.
    ///
    /// The `dev` feature leaves a `DEBUG` event on every tick it discards a
    /// scratch on, and collecting those would make every assertion below about
    /// which build this is. What these tests are about is that a request this
    /// runtime cannot serve is loud, and "loud" means a level somebody's
    /// subscriber is filtered at.
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        lock(&self.0.events).push(Recorded {
            name: event.metadata().name().to_owned(),
            level: event.metadata().level().to_string(),
            fields: fields.0,
        });
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// Collects an event's fields as text.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    /// Strings without the quotes a `Debug` would add.
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }
}

/// Held for the length of any test that emits a warning, whether or not it
/// collects one.
///
/// `tracing` caches, per callsite, whether anybody is interested in it, and the
/// cache is global to the process while a recording subscriber is thread-local.
/// So two tests in this binary that reach the same `warn!` — one under
/// [`traced`] and one not — race to write that cache, and the run where the
/// unsubscribed one wins is a run where the subscribed one collects nothing.
/// It was seen once, in a cold `--release` run of the whole workspace suite,
/// and never in this binary run on its own — and when it fails it fails in the
/// collecting test, which is a test failing for a reason that has nothing to do
/// with what it names.
///
/// Serializing them is the fix rather than rebuilding the interest cache,
/// because a rebuild narrows the window and does not close it: the losing write
/// can land after it.
static WARNINGS: Mutex<()> = Mutex::new(());

/// Takes that lock, ignoring a poisoning for the reason [`lock`] does.
fn one_warning_at_a_time() -> MutexGuard<'static, ()> {
    lock(&WARNINGS)
}

/// Runs `body` with a recording subscriber installed on this thread, and with
/// no other test in this binary emitting a warning while it does.
fn traced(body: impl FnOnce()) -> Arc<Log> {
    let held = one_warning_at_a_time();
    let log = Arc::new(Log::default());
    tracing::subscriber::with_default(Recorder(Arc::clone(&log)), body);
    drop(held);
    log
}
