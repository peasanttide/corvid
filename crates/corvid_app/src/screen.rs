//! The backend with a device behind it.
//!
//! One type for both a window and a texture, because [`corvid_render`] already
//! has one renderer for both. What this adds is the three things a run needs
//! around it: a frame count and the capture a
//! headless run would have written.

use corvid_input::Viewport;

use corvid_behavior::{Loading, Time};
use corvid_camera::Camera;
use corvid_fixed::Factor16;
use corvid_render::{Render, Renderer};
use corvid_replay::LevelRef;
use corvid_sound::AudioFrame;
// Only a windowed run resizes: a window changes size and an offscreen texture
// does not, so the one caller of `resize` is behind the same feature it is.
#[cfg(feature = "window")]
use corvid_render::Extent;
use corvid_time::Tick;

use crate::{Error, backend::Backend, capture::Capture};
use corvid_behavior::State;

/// Where a displayed frame goes when there is a device to draw it with.
///
/// # The audio frame is dropped
///
/// Nothing here hands an [`AudioFrame`] to a device. A capture still records
/// one — the frame is data and writing it down costs nothing — but nothing in
/// this crate turns it into samples, so a windowed run driven from here is
/// silent.
///
/// # A windowed run captures no picture
///
/// A read-back needs a texture that is still there after the frame was drawn,
/// and a presented surface texture belongs to the compositor. So a windowed
/// run's capture holds the same files a headless run's does and no PNG, and
/// [`App::offscreen`](crate::App::offscreen) is the run that produces one.
///
/// # Why this asks for `Render` and its `Backend` implementation asks for
/// `Present`
///
/// What a screen *is* needs only [`Render`]: a device, a target, and the
/// pipelines [`setup`](Render::setup) built, which [`draw`](Render::draw) is
/// handed back. Nothing on this type reads an input, extracts a sound or asks
/// for a cursor. What makes it a *backend* is the loop, and the loop plays a
/// [`Present`] — so the two bounds are written where each is true rather than
/// the stronger one being pushed up onto the type.
///
/// # The derived `Debug`
///
/// Derived rather than written out, which it could not be while the game's
/// graphics were a trait object with no bound: every field prints, including
/// the pipelines, because [`Render::Graphics`] is [`Debug`].
///
/// The bound the derive generates is `G: Debug` rather than the
/// `G::Graphics: Debug` it actually needs — that is what `derive` does with a
/// type parameter, and it is wrong in shape rather than in effect here. Every
/// `Render` implementation is a marker type, which is the one shape that
/// always derives [`Debug`] in a line, and nothing in this crate asks for
/// `Screen<S>: Debug` anyway. A game whose marker somehow is not `Debug` gets a
/// `Screen` that is not either, and notices nothing.
#[derive(Debug)]
pub(crate) struct Screen<S: State> {
    /// The device and the target.
    renderer: Renderer,
    /// Which game this draws, which is a bound rather than a value: the
    /// renderer holding the pipelines is the runtime's now, and what is left
    /// here is a device.
    game: core::marker::PhantomData<fn() -> S>,
    /// Where to write, if anywhere.
    capture: Option<Capture>,
    /// How many frames have arrived.
    frames: u64,
    /// The sound card, if there is somebody in front of the run to hear it and
    /// the machine would open one.
    ///
    /// [`None`] is silence and is never an error: a machine with no output
    /// device, one whose device refused, and a build with no device backend
    /// compiled in are all machines that play the game without sound rather
    /// than machines that cannot play it. **That is why there is no `cfg` on
    /// this field.** `corvid_audio` answers
    /// [`Unavailable`](corvid_audio::Unavailable) when it was built without its
    /// `device` feature, which is the same answer it gives a machine with no
    /// speakers — so the `window` feature decides whether a sound card *can* be
    /// opened, this code does not have to know which build it is in, and the
    /// two answers it can get are the same answer.
    ///
    /// **A run nobody is watching never opens one at all**, and that is decided
    /// here rather than by a feature. A headless run has no `Screen` and so
    /// cannot; an offscreen run writing a capture has one and is told not to.
    /// Both are runs whose sound would go into an empty room, and a determinism
    /// check that woke the sound card would be a determinism check with a side
    /// effect. So the only caller that passes `true` is `windowed.rs` — which
    /// is the whole of why `corvid_audio`'s `device` feature is turned on by
    /// `window` and by nothing else.
    audio: Option<corvid_audio::Audio>,
}

/// Opens the sound card, reporting rather than failing.
///
/// A machine with no output device is a machine that plays without sound. The
/// alternative — refusing to start — would make a game unplayable over a remote
/// desktop, in a container, and on any build machine, none of which is a thing
/// the player asked about.
///
/// The catalogue is the default one, which is **not silence**: an identifier no
/// game described is played as a knock at a pitch derived from its number, so a
/// cue a game fired is a cue the player hears. `Catalogue`'s own documentation
/// argues that, and it is why wiring the device needed no new trait method.
///
/// A build without `corvid_audio`'s `device` feature — which is any build
/// without `window` — reports the same thing a machine with no speakers does,
/// so this function is compiled either way and the run is silent either way.
fn open_audio() -> Option<corvid_audio::Audio> {
    match corvid_audio::Audio::open(corvid_audio::Catalogue::new()) {
        Ok(audio) => {
            tracing::info!(name: "corvid_app.audio", "the sound card is open");
            Some(audio)
        }
        Err(why) => {
            tracing::warn!(
                name: "corvid_app.no_audio",
                error = %why,
                "no audio device on this machine, so this run is silent and                  everything else is unaffected",
            );
            None
        }
    }
}

impl<S: State> Screen<S> {
    /// A backend drawing with `renderer` and writing into `capture`.
    ///
    /// The game's `setup` runs here, which is the one place it can: the device
    /// exists and the first frame has not happened. It runs exactly once, and
    /// that is what makes the renderer an owned value rather
    /// than something every frame checks for.
    ///
    /// A platform that resumes an application twice — Android hands a window
    /// back after the app has been in the background — needs a second device
    /// and therefore a second `Screen`, which is a second call to this. Nothing
    /// in this workspace targets such a platform, and `Windowed::attach`
    /// returns early rather than opening a second one, so it is a gap rather
    /// than a design.
    ///
    /// `heard` is whether there is somebody in front of this run to hear it.
    /// A windowed run is the only thing that passes `true`: an offscreen run
    /// writing a capture has an adapter and no player, and opening a sound card
    /// to play a bounce into an empty room is a side effect nobody asked for.
    pub(crate) fn new(renderer: Renderer, capture: Option<Capture>, heard: bool) -> Self {
        Self {
            renderer,
            game: core::marker::PhantomData,
            capture,
            frames: 0,
            audio: heard.then(open_audio).flatten(),
        }
    }

    /// Tells the renderer the target changed size.
    ///
    /// Called by whoever is watching the window's published state rather than
    /// from inside a frame, because this crate does not depend on
    /// `corvid_window` unless the `window` feature is on and an offscreen run
    /// has no window to watch.
    #[cfg(feature = "window")]
    pub(crate) fn resize(&mut self, size: Extent) {
        self.renderer.resize(size);
    }
}

impl<S: State, R: Render<S>> Backend<S, R> for Screen<S> {
    /// The renderer's target, which a windowed run's
    /// [`resize`](Self::resize) keeps in step with the window and an offscreen
    /// run fixed when it opened.
    fn viewport(&self) -> Option<Viewport> {
        let size = self.renderer.size();
        Some(Viewport::new(size.width, size.height))
    }

    fn present(
        &mut self,
        at: Tick,
        graphics: Option<&mut R>,
        camera: &Camera,
        loading: Option<Loading<'_, LevelRef<S>>>,
        time: Time,
        alpha: Factor16,
        audio: &AudioFrame,
    ) -> Result<(), Error> {
        // Heard before it is drawn, because the mixer runs on a thread the
        // operating system owns and the sooner a note is queued the sooner it
        // starts: drawing first would delay every sound by the time it takes to
        // rasterise a frame. `Audio::hear` is idempotent per cue — `Heard` is
        // what stops the ten displayed frames that can see one tick from
        // playing its bounce ten times — so this is safe to call every frame
        // whatever the display is doing.
        if let Some(sound) = self.audio.as_mut() {
            sound.hear(audio);
        }

        // The renderer holds the extracted pair already, so what crosses into
        // the closure is the weight between them rather than the states
        // themselves.
        // A screen without a renderer is a run that asked for an adapter and
        // a game that draws nothing: the surface is still acquired and still
        // presented, so the window is not a frozen rectangle, and the pass
        // that would have drawn is simply not recorded.
        let Some(graphics) = graphics else {
            return Ok(());
        };
        let drawn = self
            .renderer
            .frame(|target| graphics.draw(target, camera, loading, time, alpha))
            .map_err(Error::Drew)?;

        // Counted after the fact, and only when something was drawn. A
        // minimised or fully occluded window hands back no texture, so the
        // game's `draw` never ran and there is no displayed frame to count —
        // incrementing anyway made `Progress::frames` climb at full rate while
        // nothing was on screen, and wrote a capture row per frame that was
        // never displayed.
        if !drawn {
            return Ok(());
        }
        self.frames = self.frames.saturating_add(1);

        let Some(capture) = self.capture.as_ref() else {
            return Ok(());
        };
        // `read_back` answers `NotOffscreen` on a windowed run, which is not a
        // failure: a presented frame belongs to the compositor, and the rest of
        // the capture is still written.
        let png = match self.renderer.read_back() {
            Ok(image) => Some(image.to_png().map_err(Error::Drew)?),
            Err(corvid_render::Error::NotOffscreen) => None,
            Err(why) => return Err(Error::Drew(why)),
        };
        capture.frame(at, png.as_deref(), audio)
    }

    fn frames(&self) -> u64 {
        self.frames
    }

    fn capture(&self) -> Option<&Capture> {
        self.capture.as_ref()
    }
}
