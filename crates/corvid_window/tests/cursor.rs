//! What the platform actually does when a game asks for the pointer.
//!
//! `Controller::cursor` is a *request*, and the whole design rests on the runtime
//! reporting back what took rather than assuming it was granted. That reporting
//! is the thing nothing checked: every other test in this workspace that
//! touches the cursor asserts what the *game* asked for, which is one side of a
//! boundary whose other side is a windowing library and an operating system.
//!
//! So this opens a real window and asks for each of the four modes in turn.
//!
//! # Why there is no test harness
//!
//! `harness = false`, so this is a `main` and runs on the process's **main
//! thread**. `libtest` runs every `#[test]` on a worker, and an event loop
//! built off the main thread is a panic on Windows and macOS -- `Config::any_thread`
//! is an X11 and Wayland concession and says so at length. A cursor test that
//! could only run on Linux would be a cursor test that never ran where the
//! cursor behaves differently.
//!
//! One window and one loop for all four requests, because a process gets one
//! event loop.
//!
//! # When it does not run
//!
//! **By default.** `cargo test` opens no window: this asks to be asked, through
//! `CORVID_WINDOWED_TESTS`. A suite that opened windows is a suite nobody can
//! run while doing something else, one that steals focus from whatever the
//! person is typing into, and one that behaves differently on a build machine
//! with no display than on the desk it was written at. What is left running
//! everywhere is the headless half -- `an_unfocused_or_hidden_window_may_not_
//! hold_the_pointer` in `src/run.rs` -- which is the rule this crate decides;
//! what this adds is what the *platform* does with a request, and that is worth
//! a deliberate run rather than an ambush.
//!
//! On a machine with no display server it prints why it stopped and exits zero
//! even when it was asked.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::unwrap_used,
    reason = "this is a test binary with no harness, so reporting is what it does and a failed assertion is a failed test"
)]

use corvid_input::Cursor;
use corvid_input::Input;
use corvid_window::{Attached, Config, Flow, Host};
/// How many frames each request is held for.
///
/// More than one, because the request is made *after* a frame: the answer to
/// what was asked on one frame arrives in the snapshot on the next.
const FRAMES: usize = 4;

/// The four, in the order they are asked for.
const WANTED: [Cursor; 4] = [
    Cursor::Free,
    Cursor::Locked,
    Cursor::Hidden,
    Cursor::Confined,
];

/// A game that walks the four modes and records what each one came to.
struct Asking {
    /// Which frame it is on.
    frame: usize,
    /// What the snapshot reported for each request, in order.
    took: Vec<(Cursor, Cursor)>,
    /// Whether the window ever reported gaining focus, and holding it.
    focus: (bool, bool),
}

impl Asking {
    /// What is being asked for on this frame.
    fn wanted(&self) -> Cursor {
        WANTED[(self.frame / FRAMES).min(WANTED.len() - 1)]
    }
}

impl Host for Asking {
    type Error = std::convert::Infallible;

    fn attach(&mut self, _attached: &Attached) -> Result<Flow, Self::Error> {
        Ok(Flow::Go)
    }

    fn frame(&mut self, input: &Input) -> Result<Flow, Self::Error> {
        // What the previous frame's request came to, which is where the runtime
        // writes it. Only the last frame of each run of `FRAMES` is recorded, so
        // what is compared is a request that has settled rather than one still
        // arriving.
        if self.frame % FRAMES == FRAMES - 1 {
            self.took.push((self.wanted(), input.cursor()));
        }
        // A window that just opened is a window the player is looking at, and
        // a game that takes the pointer on focus depends on the platform
        // saying so.
        self.focus.0 |= input.focus().pressed;
        self.focus.1 |= input.focus().held;
        self.frame += 1;
        Ok(if self.frame < WANTED.len() * FRAMES {
            Flow::Go
        } else {
            Flow::Stop
        })
    }

    fn cursor(&self) -> Cursor {
        self.wanted()
    }
}

/// The variable that asks for this test.
///
/// Named for what it turns on rather than off, so that a person reading a CI
/// file sees a window being asked for rather than a safety being removed.
const ASKED: &str = "CORVID_WINDOWED_TESTS";

fn main() {
    if std::env::var_os(ASKED).is_none_or(|value| value.is_empty()) {
        println!("skipped: this test opens a window; set {ASKED}=1 to run it");
        return;
    }
    let host = Asking {
        frame: 0,
        took: Vec::new(),
        focus: (false, false),
    };
    let (settled, focus) = match corvid_window::run(Config::new("cursor", &[]), host) {
        Ok(host) => (host.took, host.focus),
        Err(corvid_window::Error::Opening(why)) => {
            eprintln!("skipped: this machine has no window to open ({why})");
            return;
        }
        Err(corvid_window::Error::Host(why)) => match why {},
    };

    let mut failures = 0usize;
    let mut check = |claim: &str, held: bool| {
        if held {
            println!("ok: {claim}");
        } else {
            eprintln!("FAILED: {claim}");
            failures += 1;
        }
    };

    for (wanted, took) in &settled {
        let (wanted, took) = (*wanted, *took);
        // Whatever the platform did about the *grab*, the visibility half of a
        // request is not a permission anywhere and must always take. That is
        // what `Surface::set_cursor` documents, and it is what a player reads as
        // the lock working: an invisible pointer.
        check(
            &format!(
                "{wanted:?} asked for a {} pointer and got a {} one",
                if wanted.is_visible() {
                    "visible"
                } else {
                    "hidden"
                },
                if took.is_visible() {
                    "visible"
                } else {
                    "hidden"
                }
            ),
            took.is_visible() == wanted.is_visible(),
        );

        // And the grab is either what was asked for or something further down
        // the documented fallback, never something stronger and never nothing
        // when a weaker mode was available.
        if wanted.is_grabbed() {
            check(
                &format!("{wanted:?} grabbed the pointer, and reported {took:?}"),
                took.is_grabbed(),
            );
        }
        check(
            &format!("{wanted:?} reported {took:?}, which is itself or a fallback of it"),
            took == wanted
                || core::iter::successors(Some(wanted), |mode| mode.fallback())
                    .any(|mode| mode == took),
        );
    }

    check(
        &format!(
            "all four modes were asked for, and {} were reported",
            settled.len()
        ),
        settled.len() == WANTED.len(),
    );
    // Focus is the one thing here a test may not *demand*. Whether a window
    // this process opened ends up with the player's attention is a fact about
    // the desktop it opened on -- another test's window, a screen lock, or a
    // person doing something else all take it away, and none of those is a
    // defect. So what is checked is the invariant, which holds either way:
    // focus cannot be held without having been gained.
    match focus {
        (_, false) => println!(
            "note: this window never got focus, so the focus edges were not              exercised -- something else on this desktop has it",
        ),
        (gained, true) => check(
            "the window held focus, and reported the frame it gained it",
            gained,
        ),
    }

    assert!(failures == 0, "{failures} cursor claims did not hold");
    println!("all {} cursor claims held", settled.len() * 3);
}
