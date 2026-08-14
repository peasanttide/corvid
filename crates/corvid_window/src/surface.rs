//! The window, as the two things anybody outside this crate needs it for: a
//! handle a renderer can make a surface out of, and a size.

use std::sync::Arc;

use corvid_input::Cursor;
use winit::window::CursorGrabMode;

use winit::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::state::Size;

/// A window, shareable, and safe to hand to a renderer.
///
/// # Why this is a handle rather than the window
///
/// `wgpu`'s safe surface constructor takes a window *by value* and keeps it for
/// the surface's life, which is how the lifetime obligation that its unsafe
/// constructor puts on the caller -- that the window outlive the surface -- is
/// discharged by ownership instead. That only works if the window is
/// shareable, so this is an `Arc` and a clone of it is what goes to the
/// renderer. `unsafe_code` is forbidden in this workspace and nothing here
/// needed an exception.
///
/// What this type deliberately does not expose is the rest of `winit::Window`.
/// A game that could reach the window could resize it, grab the cursor and
/// change its title from inside a frame, and the whole point of publishing
/// [`SurfaceState`](crate::SurfaceState) is that those go the other way.
#[derive(Clone, Debug)]
pub struct Surface {
    /// The window itself.
    window: Arc<winit::window::Window>,
}

impl Surface {
    /// Wraps a window the event loop just created.
    pub(crate) const fn new(window: Arc<winit::window::Window>) -> Self {
        Self { window }
    }

    /// How big the drawable area is, in physical pixels.
    #[must_use]
    pub fn size(&self) -> Size {
        let size = self.window.inner_size();
        Size::new(size.width, size.height)
    }

    /// Puts the pointer into a mode, or into the strongest one the platform
    /// will accept.
    ///
    /// Returns what actually took. Pointer grabbing is a permission in a
    /// browser, a protocol extension on Wayland and a compositor's choice
    /// elsewhere, so this walks [`Cursor::fallback`] rather than failing:
    /// `Locked` that is refused is tried as `Confined`, and `Confined` that is
    /// refused as `Free`, which no platform declines.
    ///
    /// Visibility is applied whatever the grab did, because hiding a pointer is
    /// not a permission anywhere -- so a game that asked for `Locked` and was
    /// given `Confined` still gets a hidden pointer if it asked for one, and
    /// the value returned says which of the two it has.
    ///
    /// `pub(crate)` on purpose. A game that could reach the window could resize
    /// it and move it as well, which is the paragraph on [`Surface`] itself;
    /// what a game asks for goes through `Host::cursor` instead, once a frame,
    /// where the loop is the only thing that touches the window.
    pub(crate) fn set_cursor(&self, wanted: Cursor) -> Cursor {
        let mut request = wanted;
        loop {
            let grab = match request {
                Cursor::Confined => CursorGrabMode::Confined,
                Cursor::Locked => CursorGrabMode::Locked,
                _ => CursorGrabMode::None,
            };
            if self.window.set_cursor_grab(grab).is_ok() {
                self.window.set_cursor_visible(request.is_visible());
                return request;
            }
            // `Free` releases the grab and has no fallback, and a platform that
            // refuses to release a grab it granted is one this crate has no
            // answer for. Reporting `Free` is the honest thing: nothing is
            // held, whatever the platform said about letting go.
            let Some(next) = request.fallback() else {
                self.window.set_cursor_visible(true);
                return Cursor::Free;
            };
            request = next;
        }
    }

    /// Asks the platform for another frame.
    ///
    /// The loop in this crate asks for one after every frame it draws, so a
    /// game does not have to. It is here for a game that stops drawing while
    /// nothing is happening and needs to start again.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

/// Forwarded to the window, which is what makes this a surface target.
impl HasWindowHandle for Surface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.window.window_handle()
    }
}

/// Forwarded likewise. Both are needed: a `wgpu` surface target is a display
/// and a window together, because on Wayland and X11 the connection is half of
/// the address.
impl HasDisplayHandle for Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.window.display_handle()
    }
}
