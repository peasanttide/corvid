//! What a game sounds like.

use corvid_behavior::{Data, Extract, State};
use corvid_camera::Camera;
use corvid_time::Time;

use crate::AudioFrame;

/// Where and when this frame is heard from.
///
/// Owned and [`Copy`], with no lifetime: a [`Camera`] is a pose and a frustum
/// and a [`Time`] is two numbers, so borrowing them would cost an ear a
/// lifetime parameter to save a copy smaller than the reference that replaced
/// it.
///
/// The frame being written is **not** in here. It is a separate argument to
/// [`hear`](Auralizer::hear), because it is the one thing on that call that is
/// borrowed mutably: keeping it out means this stays a value an ear can hold,
/// compare and pass on, rather than a borrow of the runtime that has to be
/// threaded through every helper an ear splits itself into.
///
/// No type parameter either: an ear reads the state through
/// [`Extract`](corvid_behavior::Extract), so nothing here is the game's own
/// type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hearing {
    /// Where the listener is, which is where the eye is.
    pub camera: Camera,
    /// Where the session is.
    pub time: Time,
}

impl Hearing {
    /// Ears at `camera`, at `time`.
    #[must_use]
    #[inline]
    pub const fn new(camera: Camera, time: Time) -> Self {
        Self { camera, time }
    }
}

/// What a game sounds like, and how.
///
/// One of the four types an [`App`](../corvid_app/struct.App.html) is made of,
/// and the one that is an *ear*. Like the renderer, it is implemented for its
/// own type, so the crate that owns the sounds owns the implementation.
///
/// # No device in `new`
///
/// The asymmetry with [`Render::new`](../corvid_render/trait.Render.html) is
/// real and is right. A renderer needs an open device to build pipelines
/// against; an ear writes an [`AudioFrame`] and something else plays it. So
/// this trait names no backend at all, which is what lets a headless run
/// compare the frames a session produced without opening an audio device to do
/// it.
///
/// # It cannot interpolate on the GPU, so it works from cues
///
/// The renderer pushes a pair and lets a shader lerp between them. Nothing here
/// can do that, and it does not need to: what a frame carries is *events* -- a
/// paddle hit, a wall thud, a goal -- read out of the hashed state, so two peers
/// play the same sounds on the same ticks. A game that recomputed a hit from two
/// ball positions would have to guess.
pub trait Auralizer<S: State>: Extract<S> {
    /// What a player has set: master volume, mix, whether to duck.
    type Config: Data;

    /// Whether this ear wants an audio device.
    ///
    /// `false` means the runtime opens none and never calls
    /// [`hear`](Self::hear) -- a dedicated server, a determinism check, a
    /// machine with no sound card.
    const REAL: bool = true;

    /// Build one from the player's settings.
    fn new(config: Self::Config) -> Self;

    /// The settings changed while the game was running.
    fn configure(&mut self, config: Self::Config);

    /// Fill one frame of audio.
    ///
    /// `out` is the frame to write into and `hearing` is where and when it is
    /// heard from. `hearing.camera` is whatever the controller's `look`
    /// answered, so the ears are where the eye is without either being told
    /// twice.
    ///
    /// # Every position written here is an offset in the listener's own frame
    ///
    /// Read this before writing the body. A [`Source`](crate::Source) and a
    /// [`Cue`](crate::Cue) carry a
    /// [`FinePoint`](corvid_vector::FinePoint) that is an **offset in the
    /// listener's frame**, in the workspace's right-handed +X right, +Y
    /// forward, +Z up convention. Neither type has a world-space position at
    /// all. The [`Listener`](crate::Listener) is the one thing in the frame
    /// that is world-space -- and `hearing.camera.pose` is exactly what it
    /// wants -- so this function is handed the ears and the sounds and does the
    /// subtraction and the rotation itself.
    ///
    /// A `hear` written as though a source carried a world position compiles
    /// against every type here and **does not fail loudly**. It is right at the
    /// origin, wrong by the listener's own displacement everywhere else, and
    /// silent past 32.7 km out -- so it passes at the origin, which is exactly
    /// where a test puts the camera.
    ///
    /// # Reuse
    ///
    /// `out` is the runtime's frame, handed over cleared and kept for the life
    /// of the process. Append to it; do not replace it.
    /// [`AudioFrame::clear`](crate::AudioFrame::clear) retains its vectors'
    /// capacity, so a frame no larger than the largest before it allocates
    /// nothing.
    ///
    /// ```
    /// use corvid_sound::{AudioFrame, Auralizer, Cue, Hearing, Listener, SoundId};
    /// # use corvid_behavior::{Extract, Extracting, Level, State};
    /// # use corvid_time::{Tick, Time};
    /// # use serde::{Deserialize, Serialize};
    /// #
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Only;
    /// # impl Level for Only {
    /// #     type Error = core::convert::Infallible;
    /// #     fn load(_: &str) -> Result<Self, Self::Error> { Ok(Self) }
    /// # }
    /// #
    /// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    /// # struct Game;
    /// # impl State for Game {
    /// #     const NAME: &'static str = "game";
    /// #     type Level = Only;
    /// #     type Rules = ();
    /// #     type Action = ();
    /// # }
    /// #
    /// const THUD: SoundId = SoundId(1);
    ///
    /// #[derive(Default)]
    /// struct Ears {
    ///     /// Whatever the last `extract` read out of the state.
    ///     thuds: Vec<Tick>,
    /// }
    ///
    /// # impl Extract<Game> for Ears {
    /// #     fn extract(&mut self, _extracting: Extracting<'_, Game>) {}
    /// # }
    /// #
    /// impl Ears {
    ///     /// A helper an ear splits itself into. `Hearing` is owned, so this
    ///     /// takes it by value and carries no lifetime of its own.
    ///     fn thud(&self, out: &mut AudioFrame, hearing: Hearing, on: Tick) {
    ///         out.cue(Cue::new(out.next_id(on), THUD));
    ///         let _ = hearing.time;
    ///     }
    /// }
    ///
    /// impl Auralizer<Game> for Ears {
    ///     type Config = ();
    ///
    ///     fn new((): ()) -> Self { Self::default() }
    ///     fn configure(&mut self, (): ()) {}
    ///
    ///     fn hear(&mut self, out: &mut AudioFrame, hearing: Hearing) {
    ///         // The ears go where the eye is, and the pose is the one
    ///         // world-space thing in the frame.
    ///         out.listen(Listener::new(hearing.camera.pose));
    ///         for on in &self.thuds {
    ///             self.thud(out, hearing, *on);
    ///         }
    ///     }
    /// }
    ///
    /// let mut ears = Ears { thuds: vec![Tick(97), Tick(97)] };
    /// let mut frame = AudioFrame::new();
    /// ears.hear(&mut frame, Hearing::default());
    ///
    /// // Two cues on one tick, numbered apart by the frame itself.
    /// assert_eq!(frame.cues.len(), 2);
    /// assert_ne!(frame.cues[0].id, frame.cues[1].id);
    /// ```
    fn hear(&mut self, out: &mut AudioFrame, hearing: Hearing);
}

/// A game with nothing to hear.
///
/// The default for an [`App`](../corvid_app/struct.App.html)'s ear, and the
/// whole of what a dedicated server owes: no audio device is opened and
/// [`hear`](Auralizer::hear) is never called.
impl<S: State> Auralizer<S> for () {
    type Config = ();

    const REAL: bool = false;

    fn new((): ()) -> Self {}

    fn configure(&mut self, (): ()) {}

    fn hear(&mut self, _out: &mut AudioFrame, _hearing: Hearing) {}
}
