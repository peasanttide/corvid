//! The state that comes from the window rather than from a control.
//!
//! Split from [`Input`](super::Input) because a file stays under 400 lines, and
//! this is the seam that was already there: everything in the parent is
//! addressed by an identifier a declaration handed out, and nothing here is.
//! A window has one focus, one cursor mode, one viewport and one control being
//! captured, and none of them belongs to an action set -- which is why they are
//! the four questions a snapshot answers the same way whichever set is active.

use super::Input;
use crate::cursor::Cursor;
use crate::source::Button;
use crate::value::{Digital, Viewport};

impl Input {
    /// Whether this window has the player's attention, and the two edges
    /// around it.
    ///
    /// A [`Digital`] because that is exactly the shape of it -- a level with an
    /// edge either side -- and inventing a second type spelled the same way
    /// would buy nothing:
    ///
    /// | | Means |
    /// |---|---|
    /// | `held` | the window has focus now |
    /// | `pressed` | it got focus between the last frame and this one |
    /// | `released` | it lost focus |
    ///
    /// It is **not an action**, so it is not filtered by the active set and no
    /// binding table decides it: there is nothing for a player to bind, and a
    /// game asking about focus is asking about the window rather than about
    /// something they did.
    ///
    /// # What it is for
    ///
    /// Two things, and both are about the pointer. A game that captures the
    /// pointer wants to take it back when the player returns -- `pressed` is
    /// that frame -- and wants to know it was taken away when they leave, which
    /// no key release will tell it, because the platform stops reporting those
    /// the moment focus goes. That is also why the runtime releases everything
    /// held on focus loss, and this is the same event said out loud so that a
    /// game can act on it rather than infer it.
    ///
    /// A run with no window never gains focus, which is honest: there is
    /// nothing there to be focused. A game keying its pointer off this simply
    /// never asks for it, and a headless run has no pointer either.
    #[must_use]
    #[inline]
    pub const fn focus(&self) -> Digital {
        self.focus
    }

    /// Records whether the window has focus.
    ///
    /// The platform half calls this; a game reads [`focus`](Self::focus).
    #[inline]
    pub const fn set_focus(&mut self, focus: Digital) {
        self.focus = focus;
    }

    /// Which control the player pressed this frame, whatever it is bound to.
    ///
    /// **The one place a raw control reaches a game, and it exists for exactly
    /// one screen.** Everything else here is an *action*: a game declares what
    /// it can be asked to do and never sees a key code, which is what lets a
    /// binding table sit between the two and what makes a game playable on a
    /// board it was not written for. A rebinding screen is the one thing that
    /// cannot work that way, because "press the control you want" is a question
    /// about the control and not about the action, and until this existed the
    /// screen in `cradle_ui` could only list what was already bound.
    ///
    /// [`None`] on a frame where nothing went down, and on every frame of a run
    /// with no devices under it. When several controls went down together this
    /// is the lowest of them in [`Button`]'s own order, so a frame that saw two
    /// presses reports the same one on every machine rather than whichever the
    /// platform mentioned first.
    ///
    /// A press, never a release and never a level: a screen that bound on
    /// release would bind the control the player let go of to dismiss it.
    ///
    /// **It is not filtered by the active set**, because it is not an action
    /// and belongs to no set. A game that reads this while the player is not
    /// rebinding gets whatever they last pressed, which is why it is read
    /// inside a capture mode and not beside the other queries.
    #[must_use]
    #[inline]
    pub const fn captured(&self) -> Option<Button> {
        self.captured
    }

    /// Records the control that went down this frame.
    ///
    /// The platform half calls this; a game reads [`captured`](Self::captured).
    #[inline]
    pub const fn set_captured(&mut self, control: Option<Button>) {
        self.captured = control;
    }

    /// What the pointer is actually doing.
    ///
    /// **What happened, not what was asked for.** A game requests a mode
    /// through `Controller::cursor` and the platform may decline -- pointer locking
    /// is a permission in a browser, a protocol extension on Wayland, and a
    /// compositor's choice elsewhere -- so this is where a game finds out. The
    /// runtime falls back down [`Cursor::fallback`] rather than failing, so
    /// asking for [`Cursor::Locked`] on a platform that refuses gives
    /// [`Cursor::Confined`] here rather than [`Cursor::Free`].
    ///
    /// Reading it matters for one thing above the rest: while
    /// [`Cursor::is_locked`] is true, [`pointer`](Self::pointer) stops moving
    /// and [`delta`](Self::delta) is the whole of what the mouse says. A game
    /// that assumed the lock took and steers from `pointer` has a camera that
    /// stops at the edge of the monitor.
    ///
    /// A headless run answers [`Cursor::Free`], because there is no pointer to
    /// be doing anything else.
    #[must_use]
    #[inline]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Records what the platform did with the pointer.
    ///
    /// The platform half calls this; a game reads [`cursor`](Self::cursor).
    #[inline]
    pub const fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    /// How big the rectangle the pointer is reported against is, if there is
    /// one.
    ///
    /// [`None`] for a run with no display -- a headless determinism check, a
    /// dedicated server -- and that is the honest answer rather than a
    /// placeholder size: a run with no window has no viewport, and a game that
    /// was handed a made-up one would lay its interface out for a display
    /// nobody is looking at. A game that needs a rectangle either way picks its
    /// own logical size for the [`None`] case and says so.
    #[must_use]
    #[inline]
    pub const fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    /// Records how big that rectangle is, or that there is no display.
    ///
    /// The platform half calls this; a game reads
    /// [`viewport`](Self::viewport).
    #[inline]
    pub const fn set_viewport(&mut self, value: Option<Viewport>) {
        self.viewport = value;
    }
}
