//! Opens the machine's sound card and plays a bounce four times a second.
//!
//! This is the check `cargo test` cannot do: opening a device needs a sound
//! card, and the machine this crate was written on has none. It builds the same
//! [`AudioFrame`](corvid_sound::AudioFrame) a game's `hear` would, hands it over
//! once per displayed frame at sixty hertz, and fires a cue every fifteenth of
//! those — which is what a fifteen-hertz simulation bouncing a cube off a wall
//! looks like from here.
//!
//! What to listen for: four knocks a second, each one a distinct sound rather
//! than a click or a buzz, at a steady volume, with no gaps and no repeats. A
//! cue read by four displayed frames and played four times is the failure this
//! is most likely to show, and it sounds like a flam rather than a knock.
//!
//! ```sh
//! cargo run --release -p corvid_audio --features device --example knock
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "an example a person runs to listen to has to say what it is doing and why it stopped, and the workspace's answer everywhere else — a tracing event — needs a subscriber that a person running one example has not installed"
)]

/// What this example is without the feature that opens a device.
///
/// An example is compiled by every `cargo test`, including the arm that builds
/// the workspace with no features at all, so it needs a `main` there too. It
/// says why rather than doing nothing quietly.
#[cfg(not(feature = "device"))]
fn main() {
    eprintln!("built without the `device` feature, so there is no sound card to open");
}

#[cfg(feature = "device")]
use std::time::Duration;

#[cfg(feature = "device")]
use corvid_audio::{Audio, Catalogue, Timbre};
#[cfg(feature = "device")]
use corvid_fixed::Factor16;
use corvid_sound::{AudioFrame, Cue, SoundId};
use corvid_time::Tick;
/// What a wall makes when the cube reaches it.
#[cfg(feature = "device")]
const THUD: SoundId = SoundId(2);

/// A brighter one, so that two sounds in the same run are audibly two sounds.
#[cfg(feature = "device")]
const TICK: SoundId = SoundId(3);

/// How many displayed frames a second this pretends to run at.
#[cfg(feature = "device")]
const DISPLAY: u32 = 60;

/// How many of those go by between one cue and the next.
#[cfg(feature = "device")]
const EVERY: u32 = 15;

/// How long the whole thing runs.
#[cfg(feature = "device")]
const FRAMES: u32 = DISPLAY * 6;

#[cfg(feature = "device")]
fn main() {
    let catalogue = Catalogue::new()
        .with(THUD, Timbre::knock(90.0).with_decay(0.22).with_bite(0.7))
        .with(TICK, Timbre::knock(1_200.0).with_decay(0.06).with_bite(0.2));

    let mut audio = match Audio::open(catalogue) {
        Ok(audio) => audio,
        Err(why) => {
            eprintln!("nothing to listen on: {why}");
            return;
        }
    };
    println!(
        "{} Hz, {} channels — four knocks a second for six seconds",
        audio.rate(),
        audio.channels(),
    );

    let mut frame = AudioFrame::new();
    for displayed in 0..FRAMES {
        // What a game's `hear` builds. A cue stays in the frame for as long as
        // the tick it was fired on is the current one, which is what makes
        // `Heard` the thing that decides it is played once.
        let tick = Tick(u64::from(displayed / EVERY));
        frame.clear();
        if displayed % EVERY < 4 {
            let id = frame.next_id(tick);
            let sound = if tick.0.is_multiple_of(4) { TICK } else { THUD };
            frame.cue(Cue::new(id, sound).with_gain(Factor16::from_f64(0.8)));
        }
        audio.hear(&frame);
        std::thread::sleep(Duration::from_micros(1_000_000 / u64::from(DISPLAY)));
    }

    // Whether the device was pulling, rather than whether it was audible. A
    // stream that opened and then stopped asking for samples reports no error,
    // so this is the only thing here that can tell the two apart.
    match audio.waiting() {
        Some(0) => println!("the device took every note it was given"),
        Some(left) => println!("{left} notes were never picked up — is the stream running?"),
        None => println!("the device thread was busy with the queue, which means it is running"),
    }
}
