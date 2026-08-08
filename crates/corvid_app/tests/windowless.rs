//! The one thing a window may not change.
//!
//! A windowed run differs from a headless one in two places: an input snapshot
//! is refilled from devices instead of standing still, and the game's own
//! `Render::draw` records `wgpu` calls instead of nothing happening. Neither is
//! on the path from an action log to a state, and this file is the attempt to
//! make that false.
//!
//! # What is checked here, and what is not
//!
//! Checked: that a run with a real adapter drawing every frame lands on the
//! same trace, the same log, the same state and the same requests as a run with
//! no adapter at all. `Renderer::offscreen` is the same renderer
//! `Renderer::for_window` is — the same acquire, the same encoder, the same
//! submit — so what this compares is a run with a device in it against a run
//! without one.
//!
//! # Why every test here runs through `backstop::drawing`
//!
//! Each of the three builds a `wgpu` device, and several devices built at once
//! against a software rasteriser wedge against each other — running this binary
//! forty times before that helper was used here left two of the forty alive
//! until a timeout killed them; two hundred runs with it left none. So the bodies are serialised and each has a
//! deadline, which turns that wedge from a binary that never exits into a
//! failure naming the test that was still drawing. It is the same fix
//! `corvid_render`'s `tests/offscreen.rs` already carries.
//!
//! Not checked: a run with a *window*. An event loop needs a display server and
//! a build machine does not have one, so the last step — that a window opens
//! and the digest is still this digest — is a manual check.
//! `examples/hello/README.md` says how to make it, and `cargo run -p hello --
//! --headless` prints the number to compare against.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test that is skipped has to say so where a person running the suite will see it, and the workspace's answer everywhere else — a tracing event — needs a subscriber that a test harness does not install"
)]

mod common;

use common::{Counting, Rules, Scratchpad, Tally, backstop, opening};
use corvid_app::{App, Outcome};
use corvid_hash::digest;
use corvid_render::Extent;
use corvid_time::Tick;
/// How far the runs below play.
const TICKS: u64 = 40;

/// How big the offscreen target is. Small: what is being compared is a digest,
/// and nothing here looks at a pixel.
const SIZE: Extent = Extent::new(64, 64);

/// A run with no adapter and nowhere to draw.
fn without() -> Outcome<Counting> {
    App::<Counting>::new()
        .headless()
        .opening(opening::<Tally>(Rules::quiet()))
        .until(|state: &Tally, _| state.now >= Tick(TICKS))
        .run()
        .unwrap()
}

/// The same run, with a device rasterising every frame, or [`None`] on a
/// machine with no adapter at all.
fn with() -> Option<Outcome<Counting>> {
    let run = App::<Counting>::new()
        .offscreen(SIZE)
        .opening(opening::<Tally>(Rules::quiet()))
        .until(|state: &Tally, _| state.now >= Tick(TICKS))
        .run();
    match run {
        Ok(outcome) => Some(outcome),
        Err(why) => {
            eprintln!("skipped: this machine has no adapter to render with ({why})");
            None
        }
    }
}

#[test]
fn a_run_with_a_renderer_lands_on_the_run_without_one() {
    backstop::drawing("a run with a renderer against a run without one", || {
        // Every part of the outcome, because a trace alone is also what two runs
        // that both did nothing produce. The log says the same actions were
        // recorded, the state says the same arithmetic happened, and the length
        // says the run went as far as it was told to.
        let Some(drawn) = with() else {
            return;
        };
        let dark = without();

        assert_eq!(drawn.session.marks, dark.session.marks);
        assert_eq!(drawn.session.log, dark.session.log);
        assert_eq!(digest(&drawn.state), digest(&dark.state));
        assert_eq!(drawn.state, dark.state);
        assert_eq!(drawn.exit, dark.exit);
        assert_eq!(drawn.session.last(), Tick(TICKS));
        assert_eq!(drawn.session.marks.len(), TICKS + 1);

        // And that this is a run in which something happened. A trace whose every
        // mark is the same digest is what a game that computes nothing produces,
        // and two of those agree with each other for free.
        assert_ne!(
            drawn.session.marks.get(Tick(1)),
            drawn.session.marks.get(Tick(TICKS)),
            "every tick of this run has the same digest",
        );
    });
}

#[test]
fn the_renderer_cannot_report_what_it_drew() {
    backstop::drawing("two runs drawn into different targets", || {
        // The other half, and the one a digest comparison cannot see: a
        // renderer that agreed on the trace could still be handing something
        // back that a later tick reads. It cannot, and the reason is
        // structural — `Render`'s `draw` returns nothing and
        // `Backend::present` answers `Result<(), Error>` — so what is asserted
        // here is the observable consequence: the whole outcome of a run is a
        // function of its session, and drawing the same run into a target of a
        // completely different shape leaves it identical.
        let Some(first) = with() else {
            return;
        };

        let second = App::<Counting>::new()
            .offscreen(Extent::new(17, 5))
            .opening(opening::<Tally>(Rules::quiet()))
            .until(|state: &Tally, _| state.now >= Tick(TICKS))
            .run()
            .unwrap();

        assert_eq!(first.session.marks, second.session.marks);
        assert_eq!(first.state, second.state);
    });
}

#[test]
fn a_run_with_a_renderer_is_still_a_run_that_asks_the_platform_for_things() {
    backstop::drawing("a run with a renderer that asks for things", || {
        // The requests are the one channel a tick has to the outside, and a
        // renderer must not swallow any of them. This uses the rules that ask for
        // everything, so the comparison is over a non-empty list rather than over
        // two empty ones.
        let rules = Rules {
            quit_at: Some(Tick(20)),
            save_at: Some(Tick(4)),
            read_at: Some(Tick(6)),
            cheer_at: Some(Tick(8)),
            snap_at: Some(Tick(10)),
            ..Rules::quiet()
        };
        // Both runs write their slots into a directory of their own, removed
        // when the comparison is over: the default is `./saves/NAME/`, which is
        // right for a game and would be this test leaving a file in the crate.
        let scratchpad = Scratchpad::new("windowless");
        let dark = App::<Counting>::new()
            .headless()
            .opening(opening::<Tally>(rules.clone()))
            .state(scratchpad.path())
            .run()
            .unwrap();

        let drawn = match App::<Counting>::new()
            .offscreen(SIZE)
            .opening(opening::<Tally>(rules))
            .state(scratchpad.path())
            .run()
        {
            Ok(outcome) => outcome,
            Err(why) => {
                eprintln!("skipped: this machine has no adapter to render with ({why})");
                return;
            }
        };

        assert!(!dark.requests.is_empty(), "this run asked for nothing");
        assert_eq!(
            format!("{:?}", drawn.requests),
            format!("{:?}", dark.requests)
        );
        assert_eq!(drawn.exit, dark.exit);
        assert_eq!(drawn.session.marks, dark.session.marks);
    });
}
