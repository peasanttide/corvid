//! The backend with no window, no adapter and no audio device.

use core::marker::PhantomData;

use corvid_behavior::State;
use corvid_input::Viewport;
use corvid_render::Render;

use crate::{
    Error,
    backend::{Backend, Frame},
    capture::Capture,
};

/// Where a displayed frame goes when there is nowhere to display it.
///
/// A frame arrives here as what the client-local half of a game produced and
/// this either writes it into a capture directory or counts it and drops it.
/// Only the [`AudioFrame`] is written: drawing needs a device, and the whole
/// point of this backend is that there is not one.
///
/// It is what a run with neither a window nor a renderer uses, which is every
/// run that did not ask for one whatever features are compiled in: a headless
/// run is the default and stays the default.
#[derive(Debug)]
pub(crate) struct Headless<S> {
    /// Where to write, if anywhere.
    capture: Option<Capture>,
    /// How many frames have arrived.
    frames: u64,
    /// Which game this is a backend for.
    ///
    /// The trait is generic over `G` because a backend with a device calls
    /// `Render::draw`, and this one is the case where knowing the game buys
    /// nothing.
    game: PhantomData<fn() -> S>,
}

impl<S> Headless<S> {
    /// A backend that writes into `capture`, or nowhere if there is none.
    pub(crate) const fn new(capture: Option<Capture>) -> Self {
        Self {
            capture,
            frames: 0,
            game: PhantomData,
        }
    }
}

impl<S: State, R: Render<S>> Backend<S, R> for Headless<S> {
    /// Nothing, because there is nothing to draw into. A game's `look` is what
    /// decides what to do about that; nothing here invents a size for it.
    fn viewport(&self) -> Option<Viewport> {
        None
    }

    fn present(&mut self, frame: Frame<'_, S, R>) -> Result<(), Error> {
        self.frames = self.frames.saturating_add(1);
        self.capture
            .as_ref()
            .map_or(Ok(()), |capture| capture.frame(frame.at, None, frame.audio))
    }

    fn frames(&self) -> u64 {
        self.frames
    }

    fn capture(&self) -> Option<&Capture> {
        self.capture.as_ref()
    }
}
