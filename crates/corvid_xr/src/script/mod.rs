//! The stand-in: a headset that is a recording.
//!
//! Public API rather than a test helper, for the reason every other stand-in in
//! this workspace is: a downstream game builds its own XR golden tests out of
//! it, and XR code paths run in CI on a machine with no headset.
//!
//! **It advances on [`poll`](crate::Headset::poll) at a fixed rate and never on
//! a clock**, so a run of a thousand frames produces the same thousand poses on
//! every machine and an XR golden is a byte comparison rather than a tolerance.
//!
//! What it does **not** do is certify the runtime. An `OpenXR` session that fails
//! to create, a swapchain format the runtime does not offer, a frame the
//! compositor drops -- none of those happen to a recording, and the only thing
//! that finds them is a headset in somebody's hands. This stops the paths from
//! rotting; it does not prove them.

mod headset;
mod motion;

pub use headset::ScriptedHeadset;

use motion::{Motion, state_at, turn};

use core::time::Duration;

use corvid_fixed::{Angle32, Factor16, I16F16};

use corvid_vector::{FinePoint, GlobalFinePoint};
use serde::{Deserialize, Serialize};

use crate::{Confidence, EyeView, Hand, Passthrough, Pose, Side, State, Tracked, Views};

/// How many frames a second the built-in tracks are recorded at.
pub const RATE: u16 = 90;

/// How far apart the built-in tracks put the eyes: 64 mm.
pub const SEPARATION: I16F16 = I16F16::from_f64(0.064);

/// One recorded frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackFrame {
    /// What the session was doing.
    pub state: State,
    /// Where the head was, in stage space.
    pub head: Tracked<Pose>,
    /// Both eyes.
    pub views: Views,
    /// Both hands, left first.
    pub hands: [Tracked<Hand>; 2],
    /// Whether the room was showing.
    pub passthrough: Passthrough,
}

/// A recorded pose track.
///
/// [`corvid_wire`]-encoded, so a track recorded on a real headset is a file a
/// test can carry. The three in `tracks/` are files for exactly that reason,
/// and their digests are frozen so one that is accidentally re-recorded is a
/// red test rather than a quiet change of fixture.
///
/// ```
/// use corvid_xr::PoseTrack;
///
/// let track = PoseTrack::still(90);
/// assert_eq!(track.frames.len(), 90);
///
/// let bytes = track.encode().expect("a track encodes");
/// assert_eq!(PoseTrack::decode(&bytes).expect("and decodes"), track);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PoseTrack {
    /// How many frames a second it was recorded at.
    pub rate: u16,
    /// The frames, in order.
    pub frames: Vec<TrackFrame>,
}

impl PoseTrack {
    /// An empty track at the built-in [`RATE`]. A headset playing it is over
    /// before it starts.
    #[must_use]
    #[inline]
    pub const fn empty() -> Self {
        Self {
            rate: RATE,
            frames: Vec::new(),
        }
    }

    /// A short track a test can build with no file: the head still, both hands
    /// forward, tracked throughout.
    #[must_use]
    pub fn still(frames: u16) -> Self {
        Self::built(frames, |_| Motion::default())
    }

    /// One that loses tracking in the middle and gets it back.
    ///
    /// The middle third reports [`Confidence::Lost`] and keeps the last value
    /// it had, which is what a runtime does and what a game has to be able to
    /// draw.
    #[must_use]
    pub fn lossy(frames: u16) -> Self {
        let total = u32::from(frames);
        let mut track = Self::built(frames, |_| Motion::default());
        let (from, to) = (total / 3, total * 2 / 3);
        let mut last = TrackFrame::default();
        for (index, frame) in track.frames.iter_mut().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            if index < from {
                last = *frame;
            } else if index < to {
                frame.head = Tracked::new(last.head.value, Confidence::Lost, frame.head.at);
                frame.views = last.views;
                for (hand, kept) in frame.hands.iter_mut().zip(last.hands) {
                    *hand = Tracked::new(kept.value, Confidence::Lost, hand.at);
                }
            }
        }
        track
    }

    /// A swarm session at table scale: the head still, the right hand gripping
    /// and sweeping an arc, which is what spinning a held planet looks like.
    #[must_use]
    pub fn table(frames: u16) -> Self {
        Self::built(frames, |index| Motion {
            yaw: Angle32::from_bits(index.wrapping_mul(0x0020_0000)),
            reach: Angle32::from_bits(index.wrapping_mul(0x00C0_0000)),
            grip: Factor16::MAX,
            pinch: Factor16::MIN,
        })
    }

    /// A defender session at surface scale: the head turning as the player
    /// looks around, the right hand aiming and pinching to place.
    #[must_use]
    pub fn surface(frames: u16) -> Self {
        Self::built(frames, |index| Motion {
            yaw: Angle32::from_bits(index.wrapping_mul(0x0060_0000)),
            reach: Angle32::from_bits(index.wrapping_mul(0x0018_0000)),
            grip: Factor16::MIN,
            pinch: Factor16::from_bits(
                u16::try_from(index.wrapping_mul(0x0400) & 0xFFFF).unwrap_or(u16::MAX),
            ),
        })
    }

    /// The bytes this track is written down as.
    ///
    /// # Errors
    ///
    /// [`corvid_wire::Error`] if the encoder refuses it, which for a track of
    /// plain fixed-point values means a track larger than the encoder's own
    /// limit.
    pub fn encode(&self) -> Result<Vec<u8>, corvid_wire::Error> {
        corvid_wire::encode(self)
    }

    /// A track read back from those bytes.
    ///
    /// # Errors
    ///
    /// [`corvid_wire::Error`] if the bytes are not a track: truncated, trailing
    /// or holding a discriminant no variant has.
    pub fn decode(bytes: &[u8]) -> Result<Self, corvid_wire::Error> {
        corvid_wire::decode(bytes)
    }

    /// How long the track runs for, at its own rate.
    #[must_use]
    pub fn duration(&self) -> Duration {
        Self::stamp(
            u32::try_from(self.frames.len()).unwrap_or(u32::MAX),
            self.rate,
        )
    }

    /// Builds `frames` frames, asking `motion` where everything is on each.
    fn built(frames: u16, motion: impl Fn(u32) -> Motion) -> Self {
        let total = u32::from(frames);
        let built = (0..total)
            .map(|index| Self::frame(index, motion(index), state_at(index, total)))
            .collect();
        Self {
            rate: RATE,
            frames: built,
        }
    }

    /// One frame of a built-in track.
    fn frame(index: u32, motion: Motion, state: State) -> TrackFrame {
        let at = Self::stamp(index, RATE);
        let facing = turn(motion.yaw);
        // 1.7 m of player, standing on the stage's floor.
        let head = Pose::new(
            GlobalFinePoint::from(FinePoint::new(
                I16F16::ZERO,
                I16F16::ZERO,
                I16F16::from_f64(1.7),
            )),
            facing,
        );
        let hands = Side::ALL.map(|side| {
            let sweep = if matches!(side, Side::Right) {
                motion.reach
            } else {
                Angle32::ZERO
            };
            let aim = turn(motion.yaw.wrapping_add(sweep));
            // Half a metre of arm, a little below the eyes and out to the side.
            let arm = FinePoint::new(
                I16F16::from_f64(if matches!(side, Side::Left) {
                    -0.2
                } else {
                    0.2
                }),
                I16F16::from_f64(0.45),
                I16F16::from_f64(-0.3),
            );
            let palm = Pose::new(
                head.position()
                    .add(GlobalFinePoint::from(aim.to_basis().rotate_fine(arm))),
                aim,
            );
            Tracked::tracked(
                Hand::open(palm)
                    .gripping(if matches!(side, Side::Right) {
                        motion.grip
                    } else {
                        Factor16::MIN
                    })
                    .pinching(if matches!(side, Side::Right) {
                        motion.pinch
                    } else {
                        Factor16::MIN
                    }),
                at,
            )
        });
        TrackFrame {
            state,
            head: Tracked::tracked(head, at),
            views: Views::from_head(head, SEPARATION, EyeView::default()),
            hands,
            passthrough: Passthrough::Off,
        }
    }

    /// When frame `index` is displayed, at `rate` frames a second.
    fn stamp(index: u32, rate: u16) -> Duration {
        let rate = if rate == 0 { RATE } else { rate };
        Duration::from_nanos(u64::from(index) * 1_000_000_000 / u64::from(rate))
    }
}
