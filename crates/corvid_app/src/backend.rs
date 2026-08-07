//! Where a displayed frame goes.
//!
//! The one seam between the loop and a device, and the reason it is a trait
//! with one interesting method: **a backend takes a frame and gives nothing
//! back.** Its signature has nowhere to return a state, a tick, an action or a
//! digest, so whichever backend a run uses, the trace it records is the same
//! trace.
//!
//! That is the claim `tests/windowless.rs` checks by running the same opening
//! against both implementations and comparing the marks, and it is why a
//! renderer's errors are the only thing that crosses back — a device that
//! stopped working is a reason to stop the run and is not a value a tick can
//! read.
//!
//! # Why it takes a view and a frame rather than a picture
//!
//! A game records `wgpu` calls into an encoder, and only the backend can open
//! one — so the backend is what calls the game rather than the other way round,
//! and there is no intermediate picture for the loop to carry between them.
//! That is why this trait is generic over `G`, and why the headless
//! implementation, which calls nothing, carries a `PhantomData`.

use corvid_input::Viewport;

use corvid_sound::AudioFrame;
use corvid_time::Tick;

use corvid_behavior::{Loading, State, Time};
use corvid_camera::Camera;
use corvid_fixed::Factor16;
use corvid_render::Render;
use corvid_replay::LevelRef;

use crate::{Error, capture::Capture};

/// Somewhere a displayed frame goes.
pub(crate) trait Backend<S: State, R: Render<S>> {
    /// How big the target is, or [`None`] where there is nothing to draw into.
    ///
    /// This is the one thing that crosses back out of a backend besides an
    /// error, and it is not a value a tick can read: the loop writes it into
    /// the input snapshot, which `look` reads and `intend` is handed. A run
    /// with no target answers [`None`] rather than a made-up size, because a
    /// headless run genuinely has no viewport and a game that was told
    /// otherwise would lay its interface out for a display nobody has.
    fn viewport(&self) -> Option<Viewport>;

    /// Takes one displayed frame.
    ///
    /// `at` is the tick the newest extracted state is at, which is what a
    /// capture names its files after.
    ///
    /// The renderer arrives by mutable reference and already holds whatever
    /// [`Extract`](corvid_behavior::Extract) put in it, so nothing about the
    /// two states crosses this boundary: what does is the weight between them.
    ///
    /// # Errors
    ///
    /// Whatever the device or the filesystem said. Nothing about the game.
    #[allow(
        clippy::too_many_arguments,
        reason = "a displayed frame is a tick, a renderer, a camera, a load, a time, a weight and a sound; bundling them into a struct would be a `Frame` again, which is what this replaced"
    )]
    fn present(
        &mut self,
        at: Tick,
        graphics: Option<&mut R>,
        camera: &Camera,
        loading: Option<Loading<'_, LevelRef<S>>>,
        time: Time,
        alpha: Factor16,
        audio: &AudioFrame,
    ) -> Result<(), Error>;

    /// How many displayed frames have been handed over.
    fn frames(&self) -> u64;

    /// The capture directory, if there is one.
    fn capture(&self) -> Option<&Capture>;
}
