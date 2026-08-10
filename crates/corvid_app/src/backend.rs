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
//! That is why this trait is generic over the game, and why the headless
//! implementation, which calls nothing, carries a `PhantomData`.
//!
//! # Why the whole game and not the two halves of it a frame names
//!
//! A frame carries a renderer and a level reference, so what a backend actually
//! reads is [`Game::Render`] and [`Game::State`] — two of the five. It is
//! written over `G` anyway, because the loop that hands frames over is
//! [`Runtime<G, B>`](crate::runtime::Runtime) and a backend named by two
//! parameters would have to be re-proved equal to the runtime's game at every
//! call site. One name for one game is what the rest of this crate is arranged
//! around, and this is not the place to make an exception for two saved bounds.

use corvid_input::Viewport;

use corvid_sound::AudioFrame;
use corvid_time::Tick;

use corvid_behavior::Loading;
use corvid_camera::Camera;
use corvid_fixed::Factor16;
use corvid_time::Time;

use crate::{Error, capture::Capture, game::Game};

/// One displayed frame, as the loop hands it to a backend.
///
/// Seven things travel together — the tick, the renderer, the camera, a load in
/// progress, the session's time, the weight between the two extracted states,
/// and what to hear — and they are a struct because that is what they are: the
/// arguments of a single call that always arrive together and mean nothing
/// apart. The list was spelled out at each of the four sites for a while, under
/// an `allow(clippy::too_many_arguments)` whose reason said bundling them
/// "would be a `Frame` again". It would; that is the point.
///
/// **Nothing about the two states crosses this seam.** The renderer already
/// holds whatever [`Extract`](corvid_behavior::Extract) put in it; what goes
/// over is the weight between them.
#[derive(Debug)]
pub(crate) struct Frame<'a, G: Game> {
    /// The tick the newest extracted state is at, which is what a capture names
    /// its files after.
    pub(crate) at: Tick,
    /// The renderer, or [`None`] for a run whose game draws nothing.
    pub(crate) graphics: Option<&'a mut G::Render>,
    /// Where the eye is, which a device turns into a matrix and an ear into a
    /// listener.
    pub(crate) camera: &'a Camera,
    /// A level being loaded, for whatever draws a progress bar.
    pub(crate) loading: Option<Loading<'a>>,
    /// Where the session is.
    pub(crate) time: Time,
    /// Where the display sits between the last tick and the next.
    pub(crate) alpha: Factor16,
    /// What to hear.
    pub(crate) audio: &'a AudioFrame,
}

/// Somewhere a displayed frame goes.
pub(crate) trait Backend<G: Game> {
    /// How big the target is, or [`None`] where there is nothing to draw into.
    ///
    /// This is the one thing that crosses back out of a backend besides an
    /// error, and it is not a value a tick can read: the loop writes it into
    /// the input snapshot, which `look` reads and `action` is handed. A run
    /// with no target answers [`None`] rather than a made-up size, because a
    /// headless run genuinely has no viewport and a game that was told
    /// otherwise would lay its interface out for a display nobody has.
    fn viewport(&self) -> Option<Viewport>;

    /// Takes one displayed frame.
    ///
    /// Everything it needs is in the [`Frame`], including why the two states are
    /// not.
    ///
    /// # Errors
    ///
    /// Whatever the device or the filesystem said. Nothing about the game.
    fn present(&mut self, frame: Frame<'_, G>) -> Result<(), Error>;

    /// How many displayed frames have been handed over.
    fn frames(&self) -> u64;

    /// The capture directory, if there is one.
    fn capture(&self) -> Option<&Capture>;
}
