//! Re-records the three tracks in `tracks/`.
//!
//! The tracks are files rather than fixtures built in a test, because a track
//! recorded on a real headset is a file too -- the format is the same one, and
//! swapping one of these for a recording is a copy rather than a port. Running
//! this rewrites them from the built-in sessions, and `tests/script.rs` freezes
//! their digests, so doing it by accident is a red test.

use std::{error::Error, fs, path::Path};

use corvid_xr::PoseTrack;

/// Nine hundred frames is ten seconds at ninety, which is a session rather than
/// a burst.
const FRAMES: u16 = 900;

fn main() -> Result<(), Box<dyn Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tracks");
    fs::create_dir_all(&directory)?;
    for (name, track) in [
        ("table", PoseTrack::table(FRAMES)),
        ("surface", PoseTrack::surface(FRAMES)),
        ("lossy", PoseTrack::lossy(FRAMES)),
    ] {
        fs::write(directory.join(format!("{name}.track")), track.encode()?)?;
    }
    Ok(())
}
