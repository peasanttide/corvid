//! State into whatever a device wants.

use alloc::sync::Arc;

use corvid_time::Time;

use crate::{PlayerId, State};

/// What an extractor is handed.
///
/// One struct rather than three arguments, so that a new thing to hand over is
/// a field here and not a signature change in every implementation.
///
/// [`Copy`], because two extractors are handed the same one per frame. `Clone`
/// is derived and `Copy` is not: the derive goes by which type parameters
/// appear rather than by what the fields hold, so it would put `S: Copy` on the
/// impl -- and a game's state is behind an `Arc` here precisely so that it does
/// not have to be. `S: Clone` is no such imposition, since
/// [`Data`](crate::Data) already demands it of every state.
#[derive(Clone, Debug)]
pub struct Extracting<'a, S: State> {
    /// The state to read.
    pub state: &'a S,
    /// The level it is being played on, by the handle the session holds it
    /// by.
    ///
    /// The handle rather than the level, because a device builds from the
    /// level -- meshes, an icon atlas -- and has to know when to build again,
    /// which is every frame's question. The session makes a new handle only
    /// when a tick changed the level, so a device that keeps the one it built
    /// from answers with [`Arc::ptr_eq`]: exact while it holds it, since an
    /// address held cannot be handed out again, and free where hashing the
    /// level to ask cost more than the frame. A rollback that plays an edit
    /// again makes an equal level under a new handle, which builds once more
    /// than it needed to. Nothing a tick reads is a handle (see
    /// [`State::tick`](crate::State::tick)); a device is not a tick.
    pub level: &'a Arc<S::Level>,
    /// Where the session is.
    pub time: Time,
    /// Which seat this machine is drawing and sounding for.
    ///
    /// A renderer picks a camera with it, an auralizer picks a listener, and a
    /// heads-up display picks whose ammunition to show. Without it every
    /// extractor had to be told separately which of the players in the state
    /// was the one sitting in front of it -- so it was already being passed,
    /// just not here, and two halves of one frame could be told different
    /// things.
    ///
    /// [`None`] for a machine with nobody in a seat: a dedicated server, a
    /// spectator, a headless run recording a replay. Those draw the session
    /// rather than a point of view, and an arbitrary seat would be a worse
    /// answer than no seat.
    pub player: Option<PlayerId>,
}

impl<S: State> Copy for Extracting<'_, S> {}

/// State into whatever a device wants, once per displayed frame.
///
/// Implemented by a renderer and by an auralizer, **for their own types** --
/// which is what lets an art crate write one against a simulation crate's state
/// without an orphan-rule marker between them, and is the reason the marker
/// type could be deleted at all.
///
/// # It is not called once per tick
///
/// At most once per **displayed frame**, for the settled newest state.
///
/// - A frame that saw no tick extracts the same state again. A fifteen-hertz
///   simulation on a hundred-and-forty-four-hertz display leaves nine frames in
///   ten with no new state to take anything out of, so what an extractor does
///   every call has to be cheap, and what it does per new state or per new
///   level it does when the tick or the level's handle moved.
/// - A frame that saw eight -- a rollback re-simulating, or a catch-up after a
///   load stalled -- extracts **once**, for the newest state once the replaying
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
