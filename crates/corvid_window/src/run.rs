//! The event loop that owns `main`.

use std::sync::Arc;

use corvid_fixed::Signed16;
use corvid_input::platform::{Axis, Button, Devices};
use corvid_input::{Analog, Input};
use corvid_signal::{Emitter, channel};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use corvid_render::Icon;

use crate::host::{Attached, Config, Flow, Host};
use crate::motion::Motion;
use crate::state::{Size, SurfaceState};
use crate::surface::Surface;
use crate::translate::{key, mouse};
use corvid_input::Cursor;
/// The platform would not give us something to draw in.
///
/// Separate from [`Error`] and not generic, so that a caller which has already
/// dealt with its own half can carry this one without carrying its own error
/// type inside itself.
#[derive(Debug)]
#[non_exhaustive]
pub enum Opening {
    /// The platform would not give us an event loop, or the loop itself
    /// failed. On a machine with no display server this is what that looks
    /// like.
    Loop(winit::error::EventLoopError),
    /// The platform gave us a loop and would not open a window.
    Window(winit::error::OsError),
}

impl std::fmt::Display for Opening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loop(why) => write!(f, "this platform has no event loop for us: {why}"),
            Self::Window(why) => write!(f, "this platform would not open a window: {why}"),
        }
    }
}

impl std::error::Error for Opening {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loop(why) => Some(why),
            Self::Window(why) => Some(why),
        }
    }
}

/// A window could not be opened, or a host stopped with a reason.
///
/// Exhaustive, unlike most of this workspace's error types, because the two
/// arms are the two halves of the program rather than a list of things that
/// can go wrong: anything else that ever fails is one side's or the other's.
/// A caller that has to tell them apart — and the whole point of the split is
/// that it does — should not have to write a wildcard for a third.
#[derive(Debug)]
pub enum Error<E> {
    /// The platform's half.
    Opening(Opening),
    /// The host stopped with a reason of its own.
    Host(E),
}

impl<E: std::fmt::Display> std::fmt::Display for Error<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opening(why) => std::fmt::Display::fmt(why, f),
            Self::Host(why) => write!(f, "the game stopped: {why}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Opening(why) => Some(why),
            Self::Host(why) => Some(why),
        }
    }
}

/// Opens a window and drives `host` until it stops or the player closes it.
///
/// **This takes the calling thread and does not give it back until the loop
/// ends.** That is not a convenience: on iOS, Android and the web the event
/// loop is the program, and a game that kept `main` would have nowhere to
/// receive events. Writing it this way on every target means one shape rather
/// than two.
///
/// The host is handed back, because it is where a run's result is: a game
/// keeps its session and its state in the host and there is nowhere else for
/// them to come out.
///
/// # Errors
///
/// [`Error::Opening`] if the platform will not give us a loop or a window, and
/// [`Error::Host`] carrying whatever the host reported.
pub fn run<H: Host>(config: Config, host: H) -> Result<H, Error<H::Error>> {
    let event_loop =
        build_loop(config.any_thread).map_err(|why| Error::Opening(Opening::Loop(why)))?;
    // Poll rather than wait: a game draws continuously, and a loop that slept
    // until the next input event would show a still frame while the cube fell.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut driver = Driver::new(config, host);
    let looped = event_loop.run_app(&mut driver);
    // Before the loop's own error is looked at, because [`Host::detach`] is
    // documented as running whatever stopped the loop and a host that writes
    // something down on the way out has no other call that does it. Propagating
    // first would mean a platform failure at teardown — an X connection dropped
    // during the server sync `run_app` performs after the loop breaks, say —
    // also threw away every tick that did play.
    driver.host.detach();
    if let Some(why) = driver.failed {
        // The host's own error is the specific one and is what asked the loop to
        // stop; the loop's error, where there is one as well, is downstream of
        // it.
        return Err(why);
    }
    looped.map_err(|why| Error::Opening(Opening::Loop(why)))?;
    Ok(driver.host)
}

/// Builds the event loop, off the main thread if the platform allows it and
/// the caller asked.
///
/// Two functions rather than one because `with_any_thread` is a
/// platform-specific extension trait: it exists on X11, Wayland and Windows and
/// nowhere else, and importing it unconditionally would not compile on macOS.
/// The Windows one is deliberately not used — it works there and a Windows
/// game that used it would be shipping an event loop on a worker thread, which
/// is the trap `Config::any_thread` documents.
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
fn build_loop(any_thread: bool) -> Result<EventLoop<()>, winit::error::EventLoopError> {
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    use winit::platform::x11::EventLoopBuilderExtX11;

    let mut builder = EventLoop::builder();
    if any_thread {
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    }
    builder.build()
}

/// The same, on a platform where the loop must own the main thread.
#[cfg(not(all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn build_loop(_any_thread: bool) -> Result<EventLoop<()>, winit::error::EventLoopError> {
    EventLoop::new()
}

/// The window, once the platform has made one.
struct Open {
    /// What a renderer was handed.
    surface: Surface,
    /// Where window state is published.
    emitter: Emitter<SurfaceState>,
    /// The latest published state, kept so a change can be published whole.
    state: SurfaceState,
}

/// What a window in this state may do with the pointer, whatever the game
/// asked for.
///
/// **A window nobody is looking at may not hold the pointer.** A game asks for
/// [`Cursor::Locked`] because it is steering a camera with the mouse, and it
/// goes on asking every frame — it has no way to know the window was minimised,
/// alt-tabbed away from, or covered, because none of those is an input event
/// and a game's `cursor` is handed nothing but its own view. So the answer has
/// to be taken here, where the window's own state is.
///
/// Without it, a run that lost focus kept the pointer pinned to a window the
/// player could not see, and the only way out was to find the window again and
/// press whatever key the game happened to bind to letting go. A minimised
/// window is worse: there is nothing to find.
///
/// Confining is refused for the same reason and hiding is not. A hidden pointer
/// over an unfocused window is invisible over *that* window and ordinary
/// everywhere else, which is what a pointer that has left is.
const fn allowed(wanted: Cursor, state: SurfaceState) -> Cursor {
    if state.focused && !state.occluded {
        wanted
    } else {
        Cursor::Free
    }
}

/// Everything the loop holds between events.
struct Driver<H: Host> {
    /// What the window was opened as.
    config: Config,
    /// The half of the program being driven.
    host: H,
    /// The window, once it exists.
    open: Option<Open>,
    /// What the devices are doing.
    devices: Devices,
    /// What the relative axes have reported and not yet been handed over for.
    ///
    /// Separate from [`Devices`] because it is where the ceiling a binding's
    /// span implies is deferred rather than clamped; `src/motion.rs` says why
    /// that is the only thing left to do between a device's pixels and a
    /// frame's `delta`.
    motion: Motion,
    /// Every gamepad on the machine, polled once per displayed frame.
    ///
    /// Beside `devices` rather than inside it, because `corvid_input` is
    /// `no_std` and names no backend: what crosses from here into it is a
    /// `Button` and a deflection, which is the same thing a key crosses as.
    #[cfg(feature = "gamepad")]
    pads: crate::pad::Pads,
    /// The snapshot handed to the host once per frame, refilled in place.
    input: Input,
    /// What stopped the loop, if it was not the player.
    failed: Option<Error<H::Error>>,
}

impl<H: Host> Driver<H> {
    /// A driver with no window yet.
    fn new(config: Config, host: H) -> Self {
        let input = Input::new(config.sets);
        Self {
            config,
            host,
            open: None,
            devices: Devices::new(),
            motion: Motion::new(),
            #[cfg(feature = "gamepad")]
            pads: crate::pad::Pads::new(),
            input,
            failed: None,
        }
    }

    /// Records what the host answered, and stops the loop if it said so.
    fn absorb(&mut self, event_loop: &ActiveEventLoop, answer: Result<Flow, H::Error>) {
        match answer {
            Ok(Flow::Go) => {}
            Ok(Flow::Stop) => event_loop.exit(),
            Err(why) => {
                self.failed = Some(Error::Host(why));
                event_loop.exit();
            }
        }
    }

    /// Publishes the window's state, if it changed.
    ///
    /// A publication is a lock and an allocation, and a platform reports a
    /// window's size far more often than it changes, so the comparison here is
    /// what keeps a frame from paying for one.
    fn publish(&mut self, change: impl FnOnce(&mut SurfaceState)) {
        let Some(open) = &mut self.open else {
            return;
        };
        let mut next = open.state;
        change(&mut next);
        if next != open.state {
            open.state = next;
            open.emitter.set(next);
        }
    }
}

impl<H: Host> ApplicationHandler for Driver<H> {
    /// Opens the window, and on Android opens it again after the app came back.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_window_icon(self.config.icon.as_ref().and_then(icon))
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.config.size.width,
                self.config.size.height,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(why) => {
                self.failed = Some(Error::Opening(Opening::Window(why)));
                event_loop.exit();
                return;
            }
        };

        // Asked of the window rather than assumed, alongside the size beside it,
        // which is read the same way. `ScaleFactorChanged` is emitted when the
        // factor *changes*, so a window opened on a scaled display and never
        // moved off it receives none — and a literal here would publish "one
        // physical pixel per logical one" for the life of the process on every
        // display that is not at a hundred per cent.
        let factor = window.scale_factor();
        let surface = Surface::new(window);
        let inner = surface.size();
        let state = SurfaceState {
            size: inner,
            scale_milli: SurfaceState::scale_from(factor),
            focused: true,
            occluded: false,
        };
        let (emitter, watch) = channel("corvid_window.surface", state);
        self.open = Some(Open {
            surface: surface.clone(),
            emitter,
            state,
        });

        let attached = Attached {
            surface,
            state: watch,
        };
        let answer = self.host.attach(&attached);
        self.absorb(event_loop, answer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                self.publish(|state| state.size = Size::new(size.width, size.height));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.publish(|state| state.scale_milli = SurfaceState::scale_from(scale_factor));
            }
            WindowEvent::Occluded(occluded) => {
                self.publish(|state| state.occluded = occluded);
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    // The platform stops reporting releases the moment focus
                    // goes, so anything held now would stay held until the
                    // player came back and pressed it again. The unpaid motion
                    // goes with it: motion made over another window is not
                    // motion in this one.
                    self.devices.released_all();
                    self.motion.forget();
                }
                // And said out loud, rather than only inferred from everything
                // going up: a game that captures the pointer wants the frame
                // the player came back on, and no key release reports that.
                self.devices.focused(focused);
                self.publish(|state| state.focused = focused);
            }

            WindowEvent::KeyboardInput { event, .. } => {
                // What the keystroke *spells*, before what it *does*. The two
                // are separate questions: an action comes from the binding
                // table and a physical key, and text comes from the layout, the
                // modifiers and possibly an input method — which is why a key
                // with no `PhysicalKey::Code` still types, and why a bound key
                // types as well as acting.
                if event.state.is_pressed()
                    && let Some(typed) = event.text.as_deref()
                {
                    self.devices.typed(typed);
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(named) = key(code) else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => self.devices.press(Button::Key(named)),
                    ElementState::Released => self.devices.release(Button::Key(named)),
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => self.devices.press(mouse(button)),
                ElementState::Released => self.devices.release(mouse(button)),
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (round(f64::from(x)), round(f64::from(y))),
                    // A pixel delta is a touchpad, which reports far smaller
                    // numbers than a wheel's detents. Dividing by the usual
                    // pixels-per-line is what makes one scroll of each feel the
                    // same, and it is a guess rather than a measurement: there
                    // is no per-device curve here.
                    MouseScrollDelta::PixelDelta(position) => {
                        (round(position.x / 16.0), round(position.y / 16.0))
                    }
                };
                self.motion.moved(Axis::Scroll, x, y);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let size = self
                    .open
                    .as_ref()
                    .map_or_else(Size::default, |open| open.state.size);
                self.devices.point(pointer(position.x, position.y, size));
            }
            WindowEvent::CursorLeft { .. } => self.devices.point(None),

            WindowEvent::RedrawRequested => {
                // The relative axes are paid first and the snapshot taken
                // second, because paying is what puts this frame's displacement
                // where the snapshot reads it and the snapshot is what clears
                // it.
                // Everything the pads did since the last frame, before the
                // relative axes are paid and the snapshot is taken, so that a
                // pad's buttons and sticks land in the same snapshot as the
                // keyboard's.
                #[cfg(feature = "gamepad")]
                self.pads.pump(&mut self.devices);
                self.motion.pay(&self.config.bindings, &mut self.devices);
                self.devices
                    .snapshot(&self.config.bindings, &mut self.input);
                let answer = self.host.frame(&self.input);
                self.absorb(event_loop, answer);
                if let Some(open) = &self.open {
                    // The pointer is asked for *after* the frame, because the
                    // answer depends on what that frame decided: a game that
                    // just opened its menu wants the pointer back on the frame
                    // it opened it. What actually took is written into the
                    // snapshot, so the next frame reads it rather than assuming
                    // the request was granted.
                    let took = open
                        .surface
                        .set_cursor(allowed(self.host.cursor(), open.state));
                    self.input.set_cursor(took);
                    // Ask for the next one before returning, which is what makes
                    // this a loop rather than a single frame.
                    open.surface.request_redraw();
                }
            }
            _ => {}
        }
    }

    /// Raw pointer motion, which is what a camera wants.
    ///
    /// `WindowEvent::CursorMoved` reports where the pointer is after the
    /// platform has applied acceleration and stopped it at the edge of the
    /// screen, so a player sweeping the mouse into the edge of their monitor
    /// would find the camera stuck. This is the unaccelerated delta, which has
    /// no edge to reach.
    ///
    /// The vertical axis is negated, for the same reason [`pointer`] flips the
    /// absolute one and in the same place: a platform measures downwards from
    /// the top of the window and [`Analog`] is documented as positive up. This
    /// is the only conversion between the two conventions on the relative path,
    /// and without it the two paths disagreed — a game reading
    /// [`Input::delta`](corvid_input::Input::delta) pitched its camera the
    /// wrong way while the same game reading
    /// [`Input::analog`](corvid_input::Input::analog) did not.
    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta: (x, y) } = event {
            self.motion
                .moved(Axis::MouseMotion, round(x), round(y).saturating_neg());
        }
    }
}

/// A Corvid icon as the one `winit` takes.
///
/// [`None`] where the platform will not have it, which is what `winit` answers
/// for an icon it cannot build — and the window opens with the platform's own
/// icon rather than not opening, because a picture is not a reason to refuse
/// somebody a game. The refusal is said out loud, since an icon that silently
/// did not appear is a thing nobody can debug from the outside.
fn icon(icon: &Icon) -> Option<winit::window::Icon> {
    match winit::window::Icon::from_rgba(icon.to_bytes(), icon.width(), icon.height()) {
        Ok(built) => Some(built),
        Err(why) => {
            tracing::warn!(
                name: "corvid_window.icon",
                width = icon.width(),
                height = icon.height(),
                why = %why,
                "this platform would not take the game's icon, so the window \
                 opens with the platform's own",
            );
            None
        }
    }
}

/// A platform's `f64` delta as the integer device units `corvid_input` counts.
///
/// Truncating towards zero rather than rounding, so that a stream of small
/// sub-unit deltas does not accumulate into motion the player did not make. A
/// finite delta too large for an `i32` clamps; one that is not finite at all is
/// no movement, because a device that reports a `NaN` has malfunctioned rather
/// than moved a very long way.
#[allow(
    clippy::cast_possible_truncation,
    reason = "this is the boundary between a platform's f64 deltas and the integer units a binding table divides; the clamp above the cast keeps the value inside an i32, and a delta that large is a device fault rather than a movement"
)]
fn round(value: f64) -> i32 {
    if value.is_finite() {
        value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    } else {
        0
    }
}

/// Where the pointer is, as a value from `-1.0` to `1.0` on each axis.
///
/// `x` runs left to right and `y` runs bottom to top, which is the convention
/// [`Analog`] documents and the opposite of the one a window reports in.
fn pointer(x: f64, y: f64, size: Size) -> Option<Analog> {
    if size.is_empty() {
        return None;
    }
    let across = 2.0 * x / f64::from(size.width) - 1.0;
    let down = 2.0 * y / f64::from(size.height) - 1.0;
    Some(Analog::new(
        Signed16::from_f64(across.clamp(-1.0, 1.0)),
        Signed16::from_f64((-down).clamp(-1.0, 1.0)),
    ))
}

#[cfg(test)]
mod tests {
    //! The three rules that have no window in them.
    //!
    //! Everything else in this module needs an event loop, and an event loop
    //! needs a display server. `tests/` says which claims about this crate are
    //! therefore checked by hand rather than by `cargo test`.

    #![allow(
        clippy::panic,
        clippy::unwrap_used,
        reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
    )]

    use super::{Cursor, Signed16, Size, SurfaceState, allowed, pointer, round};

    #[test]
    fn the_pointer_is_centred_in_the_middle_and_y_runs_upwards() {
        // The y flip is the one that is invisible until somebody aims with the
        // mouse: a window reports downwards from the top and `Analog` is
        // documented as positive up. Both corners are asserted, because a flip
        // that was missed puts the top corner where the bottom one belongs and
        // a single corner would still be at the right distance from the middle.
        let size = Size::new(800, 600);
        assert_eq!(pointer(400.0, 300.0, size), Some(super::Analog::ZERO));

        let top_left = pointer(0.0, 0.0, size).unwrap();
        assert_eq!(top_left.x, Signed16::MIN);
        assert_eq!(top_left.y, Signed16::MAX);

        let bottom_right = pointer(800.0, 600.0, size).unwrap();
        assert_eq!(bottom_right.x, Signed16::MAX);
        assert_eq!(bottom_right.y, Signed16::MIN);
    }

    #[test]
    fn a_pointer_outside_the_window_clamps_and_a_window_with_no_area_has_none() {
        // A platform reports a position outside the window during a drag, and
        // an unclamped one would wrap through `Signed16` into the opposite
        // corner.
        let size = Size::new(800, 600);
        assert_eq!(pointer(-500.0, -500.0, size).unwrap().x, Signed16::MIN);
        assert_eq!(pointer(5000.0, 5000.0, size).unwrap().x, Signed16::MAX);
        assert_eq!(pointer(10.0, 10.0, Size::new(0, 600)), None);
    }

    /// A window nobody is looking at cannot hold the pointer.
    ///
    /// Headless on purpose: the rule is a function of two booleans, and a test
    /// that opened a window to check it would be a test most machines skip. The
    /// window-opening one beside it — `tests/cursor.rs` — is about what the
    /// *platform* does with a request, which is the other half and cannot be
    /// checked this way.
    #[test]
    fn an_unfocused_or_hidden_window_may_not_hold_the_pointer() {
        let looking = SurfaceState {
            focused: true,
            occluded: false,
            ..SurfaceState::default()
        };
        let away = SurfaceState {
            focused: false,
            ..looking
        };
        let hidden = SurfaceState {
            occluded: true,
            ..looking
        };

        for wanted in [
            Cursor::Free,
            Cursor::Hidden,
            Cursor::Confined,
            Cursor::Locked,
        ] {
            // What a game asks for is what a window somebody is looking at
            // does.
            assert_eq!(allowed(wanted, looking), wanted);
            // And a window they are not gives the pointer back, whatever the
            // game goes on asking for — because it does go on asking: a game
            // has no way to know it lost focus, and its `cursor` is handed
            // nothing but its own view.
            assert_eq!(allowed(wanted, away), Cursor::Free);
            assert_eq!(allowed(wanted, hidden), Cursor::Free);
        }
    }

    #[test]
    fn a_delta_that_is_not_a_number_is_no_movement() {
        // A platform that reports a NaN delta — which happens on a device that
        // was unplugged mid-motion — would otherwise cast into something
        // arbitrary and jerk the camera.
        assert_eq!(round(f64::NAN), 0);
        assert_eq!(round(f64::INFINITY), 0);
        // A finite delta too large to count is a clamp rather than a zero,
        // because it is still a direction the player moved in.
        assert_eq!(round(1e30), i32::MAX);
        assert_eq!(round(-1e30), i32::MIN);
        assert_eq!(round(-3.7), -3);
        assert_eq!(round(3.7), 3);
    }
}
