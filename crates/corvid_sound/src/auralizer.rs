//! What a game sounds like.

use corvid_behavior::{Data, Extract, State};
use corvid_camera::Camera;
use corvid_time::Time;

use crate::AudioFrame;

/// What an ear is handed for one frame.
///
/// No type parameter: an ear reads the state through
/// [`Extract`](corvid_behavior::Extract) and writes cues here, so nothing in
/// this struct is the game's own type.
#[derive(Debug)]
pub struct Hearing<'a> {
    /// The frame to write cues into.
    pub out: &'a mut AudioFrame,
    /// Where the listener is, which is where the eye is.
    pub camera: &'a Camera,
    /// Where the session is.
    pub time: Time,
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
/// can do that, and it does not need to: what a frame carries is *events* — a
/// paddle hit, a wall thud, a goal — read out of the hashed state, so two peers
/// play the same sounds on the same ticks. A game that recomputed a hit from two
/// ball positions would have to guess.
pub trait Auralizer<S: State>: Extract<S> {
    /// What a player has set: master volume, mix, whether to duck.
    type Config: Data;

    /// Whether this ear wants an audio device.
    ///
    /// `false` means the runtime opens none and never calls
    /// [`hear`](Self::hear) — a dedicated server, a determinism check, a
    /// machine with no sound card.
    const REAL: bool = true;

    /// Build one from the player's settings.
    fn new(config: Self::Config) -> Self;

    /// The settings changed while the game was running.
    fn configure(&mut self, config: Self::Config);

    /// Fill one frame of audio.
    ///
    /// `hearing.camera` is whatever the controller's `look` answered, so the
    /// ears are where the eye is without either being told twice.
    ///
    /// # Every position written here is an offset in the listener's own frame
    ///
    /// Read this before writing the body. A [`Source`](crate::Source) and a
    /// [`Cue`](crate::Cue) carry a
    /// [`FinePoint`](corvid_vector::FinePoint) that is an **offset in the
    /// listener's frame**, in the workspace's right-handed +X right, +Y
    /// forward, +Z up convention. Neither type has a world-space position at
    /// all. The [`Listener`](crate::Listener) is the one thing in the frame
    /// that is world-space — and `hearing.camera.pose` is exactly what it
    /// wants — so this function is handed the ears and the sounds and does the
    /// subtraction and the rotation itself.
    ///
    /// A `hear` written as though a source carried a world position compiles
    /// against every type here and **does not fail loudly**. It is right at the
    /// origin, wrong by the listener's own displacement everywhere else, and
    /// silent past 32.7 km out — so it passes at the origin, which is exactly
    /// where a test puts the camera.
    ///
    /// # Reuse
    ///
    /// `hearing.out` is the runtime's frame, handed over cleared and kept for
    /// the life of the process. Append to it; do not replace it.
    /// [`AudioFrame::clear`](crate::AudioFrame::clear) retains its vectors'
    /// capacity, so a frame no larger than the largest before it allocates
    /// nothing.
    fn hear(&mut self, hearing: Hearing<'_>);
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

    fn hear(&mut self, _hearing: Hearing<'_>) {}
}
