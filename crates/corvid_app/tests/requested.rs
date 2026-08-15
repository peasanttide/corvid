//! The requests the runtime does not act on, and the record it keeps of them.
//!
//! The seam against `commands.rs` is what the loop does: those are the
//! requests it carries out, and these are the ones it records and reports.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use std::fs;

use common::{Attending, Counting, FAREWELL, Rules, Scratchpad, Tally, attendance, opening, seat};
use corvid_app::Command;
use corvid_app::{Answer, App};
use corvid_behavior::{ExitCode, Presence, ProfileId};
use corvid_replay::Profile;
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
    // A tick may return two `Quit`s -- the vocabulary says nothing against it,
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
    // returned them -- a sink that let the second win would have taken the same
    // two requests and answered with the other status.
    let quits: Vec<&Command> = run
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

    // Tick two: the late seat arrives, in its own seat -- seat one, which is its
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
}
