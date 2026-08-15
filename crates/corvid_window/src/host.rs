//! What the event loop calls, and what it is configured with.

use corvid_input::platform::Bindings;
use corvid_input::{Cursor, Input, SetDescriptor};
use corvid_render::Icon;
use corvid_signal::Watch;

use crate::state::{Size, SurfaceState};
use crate::surface::Surface;

/// Whether the loop carries on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Flow {
    /// Keep going.
    #[default]
    Go,
    /// Close the window and return from [`run`](crate::run).
    Stop,
}

/// What the window is opened as.
#[derive(Clone, Debug)]
pub struct Config {
    /// What the title bar says.
    ///
    /// [`State::NAME`](corvid_behavior::State::NAME) is where a game
    /// states it, and the runtime is what passes it here: a game that spelled
    /// its name once for its title bar and once for the directory its saves
    /// land in would have two names.
    pub title: String,
    /// What the title bar, the dock and the task switcher show, or [`None`] to
    /// leave whatever the platform would have used -- an executable's embedded
    /// icon on Windows, a `.desktop` entry's on Linux, the bundle's on macOS.
    ///
    /// [`Render::icon`](corvid_render::Render::icon) is where a game states
    /// it.
    pub icon: Option<Icon>,
    /// How big to ask for, in **physical** pixels -- the same units
    /// [`SurfaceState::size`] reports, and what this is handed to the platform
    /// as. The platform may say otherwise, which is why nothing reads this
    /// after the window exists: the size that counts is the one in
    /// [`SurfaceState`].
    pub size: Size,
    /// The declaration whose actions the snapshot answers for.
    pub sets: &'static [SetDescriptor],
    /// Which control means which action.
    ///
    /// [`Bindings::placeholder`] over the same `sets` is what a game with no
    /// binding file uses, and it is a placeholder in the strong sense: it binds
    /// by identifier number and has no idea what any action means.
    pub bindings: Bindings,
    /// Whether to allow the event loop on a thread that is not the process's
    /// first.
    ///
    /// **A game leaves this false.** X11 and Wayland are the only platforms
    /// that permit it; macOS, iOS, Android, Windows and the web all require
    /// the loop to own the main thread, and this is ignored on every one of
    /// them rather than failing there -- which is exactly what makes it a trap
    /// for a game, because the build that works is the one nobody ships.
    ///
    /// It is here because a test harness runs a test on a worker thread, and
    /// without it the only check that a window opens at all would be a person
    /// looking at one. `examples/hello/tests/windowed.rs` sets it, and it is
    /// the only thing in this workspace that does.
    pub any_thread: bool,
}

impl Config {
    /// A window with a title, a default size, and the placeholder bindings over
    /// `sets`.
    #[must_use]
    pub fn new(title: impl Into<String>, sets: &'static [SetDescriptor]) -> Self {
        Self {
            title: title.into(),
            icon: None,
            size: Size::new(1280, 720),
            sets,
            bindings: Bindings::placeholder(sets),
            any_thread: false,
        }
    }

    /// The picture the platform shows beside the title.
    #[must_use]
    pub fn icon(mut self, icon: Option<Icon>) -> Self {
        self.icon = icon;
        self
    }

    /// The size to ask the platform for.
    #[must_use]
    pub const fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// The table between controls and actions.
    #[must_use]
    pub fn bindings(mut self, bindings: Bindings) -> Self {
        self.bindings = bindings;
        self
    }

    /// Allows the loop off the main thread, where the platform permits it.
    ///
    /// See [`any_thread`](Self::any_thread) for why a game does not call this.
    #[must_use]
    pub const fn any_thread(mut self, allowed: bool) -> Self {
        self.any_thread = allowed;
        self
    }
}

/// What the window exists for, once it exists.
#[derive(Clone, Debug)]
pub struct Attached {
    /// The window, as something a renderer can make a surface out of.
    pub surface: Surface,
    /// Where the window publishes what it is doing.
    ///
    /// Read it once per frame. Nothing in this crate calls into a game when the
    /// window changes; the change is published here and noticed when the game
    /// is ready for it.
    pub state: Watch<SurfaceState>,
}

/// The half of a program the event loop drives.
///
/// # Why the loop owns `main` rather than the game
///
/// On iOS, Android and the web the event loop *is* the program: the platform
/// calls into it and there is no way to write a `loop { }` of one's own that
/// also receives events. So [`run`](crate::run) takes the thread and hands
/// control back one frame at a time, which is a shape that works on all five
/// targets rather than on the two where a game could have kept `main`.
///
/// The direction of the calls is what keeps this from being a window that runs
/// a game. Everything the loop hands over is data -- a surface, a watch, an
/// input snapshot -- and everything it gets back is [`Flow`] and an error. It
/// cannot read a state, a tick or a digest.
pub trait Host {
    /// What this host reports when something goes wrong.
    ///
    /// Bounded by [`Error`](std::error::Error), because
    /// [`run`](crate::run)'s own failure carries one as a source and a source
    /// nobody can print or chain is not a source. Every host in this workspace
    /// already reports a `thiserror` enum.
    type Error: std::error::Error + 'static;

    /// Called once, when the window and its surface exist.
    ///
    /// This is where a renderer is built, because a renderer needs a window and
    /// a window does not exist until the platform says so -- which on Android is
    /// not at start-up and can happen again after the app is backgrounded.
    ///
    /// # Errors
    ///
    /// Whatever the host reports. [`run`](crate::run) stops and returns it.
    fn attach(&mut self, attached: &Attached) -> Result<Flow, Self::Error>;

    /// Called once per displayed frame, with what the devices did since the
    /// last one.
    ///
    /// # Errors
    ///
    /// Whatever the host reports. [`run`](crate::run) stops and returns it.
    fn frame(&mut self, input: &Input) -> Result<Flow, Self::Error>;

    /// What the pointer should be doing, asked once per frame after
    /// [`frame`](Self::frame).
    ///
    /// A **request**. The loop applies it, walks [`Cursor::fallback`] when the
    /// platform declines, and reports what actually took through the next
    /// frame's [`Input::cursor`](corvid_input::Input::cursor).
    ///
    /// It is asked after `frame` rather than before, because the answer depends
    /// on what that frame decided: a host whose game just opened its menu wants
    /// the pointer back on the frame it opened it, not on the one after.
    ///
    /// The default is [`Cursor::Free`], so a host that does not care about the
    /// pointer never has to mention it.
    fn cursor(&self) -> Cursor {
        Cursor::Free
    }

    /// Called once, after the last frame, whatever stopped the loop.
    ///
    /// It cannot fail, because it runs on the way out of a `run` that may
    /// already be carrying an error and there would be nowhere to put a second
    /// one.
    fn detach(&mut self) {}
}
