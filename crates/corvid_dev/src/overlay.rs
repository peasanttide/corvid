//! What the corner of the screen shows, and the two dumps beside it.

use core::fmt::Write as _;

use corvid_hash::{Digest, digest};
use corvid_sound::AudioFrame;
use corvid_time::Tick;

/// What the corner of the screen shows.
///
/// Every field is a number the runtime already has, and the whole thing derives
/// `Hash`, so an overlay is comparable between two peers looking at the same
/// tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Overlay {
    /// The digest of the state at [`tick`](Self::tick).
    pub digest: Digest,
    /// Which tick that is.
    pub tick: Tick,
    /// How many ticks the last rollback re-simulated.
    pub rollback_depth: u8,
    /// How many snapshots the ring is holding.
    pub snapshots: u16,
    /// How many frames have been displayed.
    pub frames: u64,
}

impl Overlay {
    /// An overlay of one state at one tick, with nothing else filled in.
    #[must_use]
    pub fn of<S: core::hash::Hash>(state: &S, tick: Tick) -> Self {
        Self {
            digest: digest(state),
            tick,
            ..Self::default()
        }
    }

    /// One line, in the order a corner of a screen reads.
    ///
    /// ```
    /// use corvid_dev::Overlay;
    /// use corvid_time::Tick;
    ///
    /// let line = Overlay::of(&7_u32, Tick(40)).line();
    /// assert!(line.starts_with("tick 40  "));
    /// assert!(line.ends_with("  rollback 0  snapshots 0  frames 0"));
    /// ```
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "tick {}  {}  rollback {}  snapshots {}  frames {}",
            self.tick.0, self.digest, self.rollback_depth, self.snapshots, self.frames
        )
    }
}

/// What is playing, what fired, and the digest of the whole frame.
///
/// Nothing in an `AudioFrame` is a float, so the digest is the same on every
/// machine and is what an audio golden is.
///
/// ```
/// use corvid_dev::dump_audio;
/// use corvid_sound::{AudioFrame, Cue, SoundId};
/// use corvid_time::Tick;
///
/// let mut frame = AudioFrame::new();
/// frame.cue(Cue::new(frame.next_id(Tick(97)), SoundId(1)));
///
/// let dump = dump_audio(&frame);
/// assert!(dump.contains("cues      1"));
/// ```
#[must_use]
pub fn dump_audio(frame: &AudioFrame) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digest    {}", digest(frame));
    let _ = writeln!(out, "gain      {}", frame.listener.gain);
    let _ = writeln!(out, "sources   {}", frame.sources.len());
    let _ = writeln!(out, "cues      {}", frame.cues.len());
    let _ = writeln!(out, "buses     {}", frame.buses.len());
    out
}
