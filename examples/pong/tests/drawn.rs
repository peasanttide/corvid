//! That the game draws, on a machine with an adapter.
//!
//! Everything else in this crate's tests is about the netcode and runs with no
//! device at all. This is the other half: a real adapter rasterising this
//! game's own pipeline, and a picture read back and looked at.
//!
//! It is deliberately **not** a frozen frame golden. `examples/headless` has
//! those, pinned to a software rasteriser, because a byte-exact comparison of a
//! picture is only meaningful against a known adapter — and what is worth
//! checking here is not that the pixels are the ones somebody blessed but that
//! the renderer draws the court, moves what moves, and does not quietly go
//! blank. A test that says that is a test any machine can run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "every test here returns a `Result` so that a failure reaches for `?` rather than unwrapping, and asserts as well — a failed assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test skipped for want of a GPU has to say so where a person running the suite will see it, and a tracing event needs a subscriber the harness does not install"
)]
#![cfg(feature = "render")]

use std::path::Path;

use corvid::Input;
use corvid::Retention;
use corvid::{Acting, App, Camera, Controller, Game, SetDescriptor, TickSpan, Updating};

use corvid::PlayerId;

use corvid::Extent;

use corvid::Clock;
use corvid_test::{Scratchpad, read_png};
use pong::{Ears, Graphics, Move, RATE, Table, action, opening};

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// How big the frames are drawn. Small: nothing here is about resolution, and
/// not square, so a projection that divided the wrong row would show.
const SIZE: Extent = Extent::new(160, 90);

/// How long the run plays. Past the serve, so the ball is moving.
const TICKS: u64 = 90;

/// Whether an error is "this machine cannot draw at all" rather than "the
/// picture was wrong".
///
/// Only the two adapter cases. Matching the whole of `Drew` would swallow a
/// failed read-back and a refused PNG encode and report both as a machine with
/// no GPU — which is the shape of a test that passes green on the regressions
/// it exists to catch.
const fn no_adapter(why: &corvid::Error) -> bool {
    matches!(
        why,
        corvid::Error::Drew(
            corvid::render::Error::NoAdapter(_) | corvid::render::Error::NoDevice(_)
        )
    )
}

/// Plays offscreen into `into`, or answers `false` if this machine has no
/// adapter.
///
/// A paddle that moves on a fixed period, so a later frame is not an earlier
/// one.
///
/// Its own controller rather than `pong::Hands`'s scripted mode, because the
/// period here is this test's and not the game's: `Hands` scripts from the seat,
/// which is what `--bot` wants and not what a test asserting on pictures wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Metronome {
    /// How many ticks a full up-and-down cycle takes.
    period: u64,
}

impl Controller<Table> for Metronome {
    type Config = u64;

    const SETS: &'static [SetDescriptor] = action::SETS;

    fn new(period: u64) -> Self {
        Self { period }
    }

    fn configure(&mut self, period: u64) {
        self.period = period;
    }

    fn action(&self, acting: Acting<'_, Table>) -> Move {
        if acting.time.tick.0 % self.period < self.period / 2 {
            Move::Up
        } else {
            Move::Down
        }
    }

    fn update(&mut self, _updating: Updating<'_, Table>) {}

    fn look(&self) -> Camera {
        Camera::default()
    }
}

/// The game this file draws: the table, a paddle on a metronome, and the whole
/// device half.
///
/// A marker of its own beside the binary's, because what this test wants at the
/// controls is a paddle that moves the same way on every machine — a keyboard
/// would make the picture a function of who is watching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Drawn;

impl Game for Drawn {
    const PERIOD: TickSpan = RATE;

    type State = Table;
    type Controller = Metronome;
    type Bot = ();
    type Render = Graphics;
    type Auralizer = Ears;
}

fn draw_into(into: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let played = App::<Drawn>::new()
        .opening(opening())
        .rate(RATE)
        .seat(PlayerId(0))
        .clock(Clock::stepping(RATE.period()))
        .input(Input::new(action::SETS))
        .settings(corvid::Settings {
            controls: 20,
            ..corvid::Settings::default()
        })
        .offscreen(SIZE)
        .capture(into.to_path_buf())
        .retain(Retention::Everything)
        .for_ticks(TICKS)
        .run();

    match played {
        Ok(_) => Ok(true),
        Err(why) if no_adapter(&why) => {
            eprintln!("skipped: this machine has no adapter to draw with ({why})");
            Ok(false)
        }
        Err(why) => Err(Box::new(why)),
    }
}

/// The court is drawn, it is not blank, and what moves moves.
#[test]
fn the_game_draws_a_court_that_changes() -> Fallible {
    let scratchpad = Scratchpad::new("pong-drawn");
    if !draw_into(scratchpad.path())? {
        return Ok(());
    }

    let frames = scratchpad.path().join("frames");
    let early = read_png(&frames.join("30.png"))?;
    let later = read_png(&frames.join("80.png"))?;

    assert_eq!(
        (early.width, early.height),
        (SIZE.width, SIZE.height),
        "the frames are not the size the run asked to draw",
    );

    // Not blank. A renderer that cleared the target and recorded nothing else
    // would pass every test in this crate that is about the netcode, and this
    // is the one that would not: the court's lines and the paddles are lighter
    // than the background, so a frame with nothing in it has one colour in it.
    let colours = early
        .pixels
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        colours.len() >= 3,
        "the frame holds {} distinct colours, which is a court nobody drew",
        colours.len(),
    );

    // And the picture is a function of the state rather than of nothing: fifty
    // ticks later the ball and a paddle have moved.
    assert_ne!(
        early.pixels, later.pixels,
        "two frames fifty ticks apart are identical, so the picture is not \
         being drawn from the state",
    );
    Ok(())
}

/// Every tick the run played left a frame behind.
///
/// The capture writes one per displayed frame, and a headless run displays one
/// per tick — so this is also the check that the offscreen path draws every
/// tick rather than the first one and then giving up.
#[test]
fn every_tick_leaves_a_frame() -> Fallible {
    let scratchpad = Scratchpad::new("pong-frames");
    if !draw_into(scratchpad.path())? {
        return Ok(());
    }

    let frames = std::fs::read_dir(scratchpad.path().join("frames"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "png"))
        .count();
    assert_eq!(
        u64::try_from(frames).unwrap_or(0),
        TICKS,
        "a run of {TICKS} ticks drew {frames} frames",
    );
    Ok(())
}
