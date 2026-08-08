//! State into whatever a device wants.

use crate::{State, Time};

/// What an extractor is handed.
///
/// One struct rather than three arguments, so that a new thing to hand over is
/// a field here and not a signature change in every implementation.
///
/// [`Copy`], because two extractors are handed the same one per frame.
///
/// Written by hand rather than derived: a derive puts `S: Copy` on the impl,
/// because it goes by which type parameters appear rather than by what the
/// fields actually hold. Every field here is a shared reference or a `Time`,
/// copy regardless of whether `S` is, and the state a game hands over is
/// behind an `Arc` precisely so that it does not have to be.
#[derive(Debug)]
pub struct Extracting<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// The level it is being played on.
    pub level: &'a S::Level,
    /// Where the session is.
    pub time: Time,
}

#[allow(
    clippy::expl_impl_clone_on_copy,
    reason = "a derive would add S: Clone, which is not true of every game's state and not needed by any field here"
)]
impl<S: State> Clone for Extracting<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: State> Copy for Extracting<'_, S> {}

/// State into whatever a device wants, once per displayed frame.
///
/// Implemented by a renderer and by an auralizer, **for their own types** —
/// which is what lets an art crate write one against a simulation crate's state
/// without an orphan-rule marker between them, and is the reason the marker
/// type could be deleted at all.
///
/// # It is not called once per tick
///
/// At most once per **displayed frame**, for the settled newest state.
///
/// - A frame that saw no tick extracts nothing. A fifteen-hertz simulation on a
///   hundred-and-forty-four-hertz display leaves nine frames in ten with no new
///   state to take anything out of.
/// - A frame that saw eight — a rollback re-simulating, or a catch-up after a
///   load stalled — extracts **once**, for the newest state once the replaying
///   has finished. Replayed ticks are never extracted individually.
///
/// The second of those has a cost that was chosen rather than overlooked: after
/// a rollback the pair a renderer holds can span more than one tick, so the GPU
/// lerps across a gap and things visibly jump. A lockstep session over a lossy
/// link rolls back several times a second, so this is visible rather than
/// theoretical. What it buys is that a frame already late enough to need a
/// rollback does not also pay for eight buffer writes.
///
/// # Interpolation is the GPU's
///
/// This pushes the pair; [`draw`](../corvid_render/trait.Render.html#tymethod.draw)
/// sets the weight and the shader lerps. Nothing on that path is hashed, sent
/// or compared against a golden, which is why an `f32` lerp is allowed there
/// and nowhere below it.
pub trait Extract<S: State> {
    /// Read out of a state whatever this half needs to draw or to sound.
    fn extract(&mut self, extracting: Extracting<'_, S>);
}

/// A device that wants nothing, which is what a dedicated server has two of.
impl<S: State> Extract<S> for () {
    fn extract(&mut self, _extracting: Extracting<'_, S>) {}
}
