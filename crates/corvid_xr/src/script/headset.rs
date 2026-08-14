//! The scripted headset: a track, played back as a [`Headset`].
//!
//! The seam against `mod.rs` is the contract. A [`PoseTrack`] is data with no
//! opinion about time, and this is what walks one frame at a time and answers
//! the trait a game holds.

use crate::script::{PoseTrack, RATE, TrackFrame};
use crate::{Hand, Haptic, Headset, Passthrough, Pose, Side, Space, State, Tracked, Views};

/// A headset that is a recording.
///
/// ```
/// use corvid_xr::{Headset, PoseTrack, ScriptedHeadset, State};
///
/// // A track records a session, so a short one is its opening: created,
/// // showing, and on a head.
/// let mut headset = ScriptedHeadset::new(PoseTrack::still(3));
/// assert_eq!(headset.poll(), State::Ready);
/// assert_eq!(headset.poll(), State::Visible);
/// assert_eq!(headset.poll(), State::Focused);
/// assert_eq!(headset.poll(), State::Exiting);
/// ```
#[derive(Clone, Debug)]
pub struct ScriptedHeadset {
    /// What is being played.
    track: PoseTrack,
    /// Which frame is showing.
    cursor: usize,
    /// How many times it has been polled, so the first poll shows frame zero.
    polls: u64,
    /// Whether it starts again rather than ending.
    looping: bool,
    /// Whether it has ended.
    ended: bool,
    /// What was last asked of [`Headset::set_passthrough`].
    wanted: Option<Passthrough>,
    /// Every rumble that was asked for, in order.
    rumbles: Vec<(usize, Haptic)>,
}

impl Default for ScriptedHeadset {
    /// Ninety still frames, which is the shortest useful session.
    #[inline]
    fn default() -> Self {
        Self::new(PoseTrack::still(RATE))
    }
}

impl ScriptedHeadset {
    /// A headset that plays `track` once.
    #[must_use]
    #[inline]
    pub const fn new(track: PoseTrack) -> Self {
        Self {
            track,
            cursor: 0,
            polls: 0,
            looping: false,
            ended: false,
            wanted: None,
            rumbles: Vec::new(),
        }
    }

    /// Play it round and round rather than ending.
    #[must_use]
    #[inline]
    pub const fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Which frame it is on.
    #[must_use]
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a track with more than four thousand million frames is thirteen thousand hours of recording; the cast is the honest width for a frame number and there is no fallible conversion in a const fn"
    )]
    pub const fn frame(&self) -> u32 {
        self.cursor as u32
    }

    /// How many times it has been polled.
    #[must_use]
    #[inline]
    pub const fn polls(&self) -> u64 {
        self.polls
    }

    /// Whether the track has run out.
    #[must_use]
    #[inline]
    pub const fn is_over(&self) -> bool {
        self.ended
    }

    /// What is being played.
    #[must_use]
    #[inline]
    pub const fn track(&self) -> &PoseTrack {
        &self.track
    }

    /// What was asked of it, so a test can assert a haptic fired.
    #[must_use]
    #[inline]
    pub fn rumbles(&self) -> &[(usize, Haptic)] {
        &self.rumbles
    }

    /// Forget the rumbles so far, so a test can look at one frame's.
    #[inline]
    pub fn clear_rumbles(&mut self) {
        self.rumbles.clear();
    }

    /// The frame showing now, or a default one before the first poll and after
    /// the last.
    fn here(&self) -> TrackFrame {
        if self.ended {
            return TrackFrame::default();
        }
        self.track
            .frames
            .get(self.cursor)
            .copied()
            .unwrap_or_default()
    }
}

impl Headset for ScriptedHeadset {
    fn poll(&mut self) -> State {
        if self.track.frames.is_empty() {
            self.ended = true;
        } else if self.polls > 0 && !self.ended {
            let next = self.cursor + 1;
            if next < self.track.frames.len() {
                self.cursor = next;
            } else if self.looping {
                self.cursor = 0;
            } else {
                self.ended = true;
            }
        }
        self.polls = self.polls.saturating_add(1);
        if self.ended {
            State::Exiting
        } else {
            self.here().state
        }
    }

    fn head(&self, space: Space) -> Tracked<Pose> {
        let here = self.here().head;
        match space {
            Space::Stage => here,
            Space::Local => {
                let opening = self
                    .track
                    .frames
                    .first()
                    .map_or(Pose::IDENTITY, |frame| frame.head.value);
                here.map(|pose| opening.inverse().compose(pose))
            }
            Space::View => here.map(|_| Pose::IDENTITY),
        }
    }

    fn views(&self) -> Tracked<Views> {
        let frame = self.here();
        Tracked::new(frame.views, frame.head.confidence, frame.head.at)
    }

    fn hands(&self) -> [Tracked<Hand>; 2] {
        self.here().hands
    }

    fn rumble(&mut self, hand: usize, effect: Haptic) {
        if Side::try_from(hand).is_ok() {
            self.rumbles.push((hand, effect));
        }
    }

    fn passthrough(&self) -> Passthrough {
        let offered = self.here().passthrough;
        match self.wanted {
            Some(wanted) if offered.is_available() => wanted,
            _ => offered,
        }
    }

    fn set_passthrough(&mut self, on: bool) -> Passthrough {
        let answer = self.here().passthrough.asked(on);
        if answer.is_available() {
            self.wanted = Some(answer);
        }
        answer
    }

    fn rate(&self) -> u16 {
        self.track.rate
    }
}
