//! Plays a MIDI file through the embedded bank on the default output device.
//!
//! The check `cargo test` cannot do, because opening a device needs a sound
//! card:
//!
//! ```sh
//! cargo run --release -p corvid_music --features device,unison --example play -- song.mid
//! ```
//!
//! What to listen for: the song, at an even level, with no crackle, from its
//! first note to its last, after which the example says it is over.

#![allow(
    clippy::print_stderr,
    reason = "an example a person runs to listen to has to say what it is doing, and a tracing subscriber is more than one example should install"
)]

/// What this example is without the features that open a device and carry a
/// bank: an example is built by every `cargo test`, so it still needs a `main`.
#[cfg(not(all(feature = "device", feature = "unison")))]
fn main() {
    eprintln!("built without `device` and `unison`, so there is nothing to play on");
}

#[cfg(all(feature = "device", feature = "unison"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use corvid_music::{Bank, Music, Song};

    let path = std::env::args().nth(1).ok_or("usage: play <file.mid>")?;
    let song = Song::parse(&std::fs::read(&path)?)?;
    eprintln!("{path}: {} events, {:.1} s", song.len(), song.seconds());
    let music = Music::open(Bank::unison)?;
    eprintln!("playing at {} Hz", music.rate());
    music.play(song);
    while music.playing() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("over");
    Ok(())
}
