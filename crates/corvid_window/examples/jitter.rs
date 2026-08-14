//! Measures how far a camera turns for a mouse sweep of a known length, both
//! ways round.
//!
//! This is the measurement behind the numbers in the README, and it is here so
//! that they can be re-run rather than believed. It opens a real window,
//! injects a perfectly steady stream of pointer motion through the X Test
//! extension, and reports how far two different `look`s would have turned the
//! camera by:
//!
//! **`delta`** is what a game writes now: the frame's displacement added as it
//! stands, with no `dt` anywhere.
//!
//! **`analog x dt`** is the bug the split exists to make unwritable: the same
//! per-frame number read as though it were a deflection and multiplied by the
//! frame's seconds. It is computed from the same recorded rows, so the two
//! columns are one measurement rather than two runs of different code.
//!
//! Two numbers come out of each. The first is the **total** turn for the sweep,
//! which has to be the same at every frame rate, because the hand moved the
//! same distance. The second is the turn accumulated in each tenth of a second
//! of the sweep, which has to be the same in every window, because the injector
//! sends the same number of pixels in each -- and that spread is what a player
//! sees as shake.
//!
//! ```sh
//! Xvfb :99 -screen 0 3000x2000x24 &
//! DISPLAY=:99 cargo run --release -p corvid_window --example jitter -- 8
//! ```
//!
//! The argument is how many milliseconds to sleep per frame, standing in for a
//! renderer; every eighth frame sleeps five times as long, which is what an
//! uneven display looks like and is what makes the second number interesting.
//! Zero is the free-running loop, which on this machine reaches 174 kHz and is
//! the case a per-frame division gets most wrong.
//!
//! It needs a display server, `python3`, and `libXtst`. Without them it says so
//! and stops, because a measurement that quietly measured nothing would be
//! worse than none.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "an example whose entire output is a table of measurements has to print it, and the workspace's answer everywhere else -- a tracing event -- needs a subscriber that a person running one example has not installed"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "every cast here is a count or an integer number of nanoseconds on its way into a printed statistic, where the last few bits of an f64 are far below what the measurement resolves"
)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use corvid_input::Input;

use corvid_window::{Attached, Config, Flow, Host};
// One analog action, which the placeholder table binds to mouse motion.
corvid_input::action_sets! {
    pub set Playing {
        analog LOOK;
    }
}

/// How long the whole run lasts.
const RUN: Duration = Duration::from_secs(4);

/// How long the injector waits before it starts, so that the window is open and
/// the loop is warm.
const SETTLE: f64 = 0.7;

/// How long it injects for.
const SWEEP: f64 = 2.0;

/// How many pixels each injected step moves.
const STEP: i32 = 4;

/// How many steps a second it injects.
const RATE: f64 = 250.0;

/// How far a full deflection would turn the camera in a second, in turns, for
/// the `analog x dt` column.
///
/// A reference sensitivity, not any particular game's. This example does not
/// depend on `hello` -- an example that depended on a game would be the wrong
/// way round -- so it names its own, and what it measures is the *shape* of the
/// two curves rather than how fast either one feels.
///
/// It was `examples/hello`'s `TURN_PER_MILLI` when that constant existed. That
/// game now names its sensitivity per full sweep rather than per millisecond
/// (`TURN_PER_FULL_DEFLECTION`, and it is 9.6 times this), so the absolute
/// degrees below are this example's own and are not that game's. Reading them
/// as "how far `hello` turns" is the one mistake this constant invites.
const TURNS_PER_SECOND: f64 = 0.25;

/// How far a full sweep of the axis turns the camera, in degrees, for the
/// `delta` column.
///
/// The same feel as [`TURNS_PER_SECOND`] had on a sixtieth-of-a-second frame,
/// which is what makes the two columns comparable: a display running at sixty
/// hertz reads the same total either way, and every other rate is where they
/// come apart.
const DEGREES_PER_SWEEP: f64 = TURNS_PER_SECOND / 60.0 * 360.0;

/// How long each window of the report covers.
const WINDOW: f64 = 0.1;

/// How many windows that makes.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "two constants written down four lines apart, divided at compile time"
)]
const WINDOWS: usize = (SWEEP / WINDOW) as usize;

/// The injector, as a program rather than a dependency.
///
/// A crate that talks to the X Test extension would be a dependency this crate
/// carries into every game that opens a window, for the sake of one example, so
/// this reaches it the way a person would from a shell.
const INJECTOR: &str = r"
import ctypes, ctypes.util, sys, time
x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
xtst = ctypes.CDLL(ctypes.util.find_library('Xtst'))
x11.XOpenDisplay.restype = ctypes.c_void_p
display = x11.XOpenDisplay(None)
if not display:
    sys.exit('the injector has no display')
display = ctypes.c_void_p(display)
settle, sweep, step, rate = (float(v) for v in sys.argv[1:5])
# Somewhere with room to move, so the pointer does not spend the sweep against
# the edge of the screen.
xtst.XTestFakeMotionEvent(display, 0, 100, 1000, 0)
x11.XFlush(display)
time.sleep(settle)
sent = 0
start = time.monotonic()
while time.monotonic() < start + sweep:
    xtst.XTestFakeRelativeMotionEvent(display, int(step), 0, 0)
    x11.XFlush(display)
    sent += 1
    delay = start + sent / rate - time.monotonic()
    if delay > 0:
        time.sleep(delay)
print(f'injected {sent * int(step)} px over {time.monotonic() - start:.3f}s', file=sys.stderr)
";

/// What one frame saw.
#[derive(Clone, Copy, Debug)]
struct Row {
    /// How long the frame took.
    dt: Duration,
    /// What the look axis read, as its raw `Signed16` bit pattern.
    ///
    /// One number for both columns, because it is one number: the fraction of a
    /// sweep this frame saw. What the two columns disagree about is what to do
    /// with it, not what it is.
    axis: i32,
}

/// Which of the two `look`s a turn is being computed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    /// The displacement added as it stands, which is what `Input::delta` is
    /// for.
    Delta,
    /// The same number multiplied by the frame's seconds, which is what
    /// reading it as a deflection would do.
    AnalogTimesDt,
}

/// A host that records rather than draws.
struct Recorder {
    /// When the last frame was.
    last: Instant,
    /// When the run started.
    opened: Instant,
    /// How long to sleep per frame, standing in for a renderer.
    sleep: Duration,
    /// One entry per frame.
    rows: Vec<Row>,
}

impl Host for Recorder {
    type Error = std::convert::Infallible;

    fn attach(&mut self, attached: &Attached) -> Result<Flow, Self::Error> {
        let _ = attached.surface.size();
        self.last = Instant::now();
        self.opened = self.last;
        Ok(Flow::Go)
    }

    fn frame(&mut self, input: &Input) -> Result<Flow, Self::Error> {
        let now = Instant::now();
        self.rows.push(Row {
            dt: now.saturating_duration_since(self.last),
            axis: i32::from(input.delta(LOOK).x.to_bits()),
        });
        self.last = now;
        if !self.sleep.is_zero() {
            // An uneven display: one long frame in eight, which is what a
            // renderer that occasionally misses looks like and is the case a
            // camera tied to the frame time shakes worst on.
            let long = self.rows.len().is_multiple_of(8);
            std::thread::sleep(self.sleep * if long { 5 } else { 1 });
        }
        Ok(if now.saturating_duration_since(self.opened) < RUN {
            Flow::Go
        } else {
            Flow::Stop
        })
    }
}

/// How far one of the two `look`s would have turned in this frame, in degrees.
fn turn(row: Row, model: Model) -> f64 {
    let fraction = f64::from(row.axis) / 32_767.0;
    match model {
        Model::Delta => fraction * DEGREES_PER_SWEEP,
        Model::AnalogTimesDt => fraction * TURNS_PER_SECOND * row.dt.as_secs_f64() * 360.0,
    }
}

/// The frames the injected sweep happened over.
///
/// The injector warps the pointer somewhere with room to move before it
/// settles, and that warp is a motion event like any other -- so the search for
/// the first frame of the sweep ignores everything in the first [`SETTLE`],
/// rather than starting at the first frame that saw anything. The injector
/// waits that long *after* it has warped and after the interpreter has started,
/// so the two cannot overlap.
fn sweep(rows: &[Row]) -> Vec<Row> {
    let mut elapsed = 0.0;
    let mut from = None;
    for (index, row) in rows.iter().enumerate() {
        elapsed += row.dt.as_secs_f64();
        if from.is_none() && elapsed > SETTLE && row.axis != 0 {
            from = Some((index, elapsed));
        }
        if let Some((first, began)) = from
            && elapsed - began > SWEEP
        {
            return rows.get(first..index).unwrap_or_default().to_vec();
        }
    }
    Vec::new()
}

/// The turn accumulated in each whole [`WINDOW`] of the sweep.
///
/// A frame's turn accrues over the frame, so a frame that straddles two windows
/// is split between them in proportion to how much of it fell in each. Putting
/// all of it in the window the frame started in would make the report a
/// measurement of where the bucket boundaries landed: at forty hertz a window
/// holds four frames, and one that happened to hold a long one would collect
/// five times its share whatever the axis had said.
fn windows(rows: &[Row], model: Model) -> Vec<f64> {
    let mut buckets = vec![0.0; WINDOWS];
    let mut elapsed = 0.0;
    for row in rows {
        let (from, to) = (elapsed, elapsed + row.dt.as_secs_f64());
        elapsed = to;
        let turned = turn(*row, model);
        if turned == 0.0 || to <= from {
            continue;
        }
        for (index, bucket) in buckets.iter_mut().enumerate() {
            let (low, high) = (index as f64 * WINDOW, (index + 1) as f64 * WINDOW);
            let overlap = to.min(high) - from.max(low);
            if overlap > 0.0 {
                *bucket += turned * overlap / (to - from);
            }
        }
    }
    buckets
}

/// Starts the injector, or says why it could not.
fn inject() -> Option<Child> {
    Command::new("python3")
        .arg("-c")
        .arg(INJECTOR)
        .arg(SETTLE.to_string())
        .arg(SWEEP.to_string())
        .arg(STEP.to_string())
        .arg(RATE.to_string())
        .spawn()
        .ok()
}

fn main() {
    let sleep_ms: u64 = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(0);

    let Some(mut injector) = inject() else {
        eprintln!("no python3 to inject pointer motion with; nothing to measure");
        return;
    };

    let host = Recorder {
        last: Instant::now(),
        opened: Instant::now(),
        sleep: Duration::from_millis(sleep_ms),
        rows: Vec::with_capacity(1 << 20),
    };
    let done = match corvid_window::run(Config::new("jitter", SETS).any_thread(true), host) {
        Ok(done) => done,
        Err(why) => {
            eprintln!("no window to measure in: {why}");
            let _ = injector.wait();
            return;
        }
    };
    let _ = injector.wait();

    let rows = sweep(&done.rows);
    let seconds: f64 = rows.iter().map(|row| row.dt.as_secs_f64()).sum();
    let pixels = f64::from(STEP) * SWEEP * RATE;

    println!(
        "{} frames in {seconds:.3}s ({:.0} Hz), sleeping {sleep_ms} ms a frame",
        rows.len(),
        rows.len() as f64 / seconds,
    );
    for (name, model) in [
        ("delta", Model::Delta),
        ("analog x dt", Model::AnalogTimesDt),
    ] {
        let total: f64 = rows.iter().map(|row| turn(*row, model)).sum();
        let mut buckets = windows(&rows, model);
        buckets.sort_by(f64::total_cmp);
        let low = buckets.first().copied().unwrap_or(0.0);
        let high = buckets.last().copied().unwrap_or(0.0);
        let median = buckets.get(buckets.len() / 2).copied().unwrap_or(0.0);
        println!("  {name:>11}: {pixels:.0} px turned the camera {total:.3} deg");
        println!(
            "               per {:.0} ms: min {low:.4} deg median {median:.4} deg max {high:.4} deg, spread {:.1}x",
            WINDOW * 1000.0,
            if low > 0.0 { high / low } else { f64::INFINITY },
        );
    }
}
