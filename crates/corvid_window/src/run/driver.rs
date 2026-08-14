//! The `winit` callbacks, and the state they share.
//!
//! The seam against `mod.rs` is ownership of the thread: `run` hands the loop
//! over and never returns, and everything here happens inside it. Nothing in
//! this file decides when the loop ends; it publishes what the window did and
//! asks the host what to do next.

use std::sync::Arc;

use corvid_input::platform::{Axis, Button, Devices};
use corvid_input::{Cursor, Input};
use corvid_signal::{Emitter, channel};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use super::convert::{icon, pointer, round};
use super::{Error, Opening};
use crate::host::{Attached, Config, Flow, Host};
use crate::motion::Motion;
use crate::state::{Size, SurfaceState};
use crate::surface::Surface;
use crate::translate::{key, mouse};

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
/// goes on asking every frame -- it has no way to know the window was minimised,
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
pub(super) const fn allowed(wanted: Cursor, state: SurfaceState) -> Cursor {
    if state.focused && !state.occluded {
        wanted
    } else {
        Cursor::Free
    }
}

/// Everything the loop holds between events.
pub(super) struct Driver<H: Host> {
    /// What the window was opened as.
    config: Config,
    /// The half of the program being driven.
    pub(super) host: H,
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
    pub(super) failed: Option<Error<H::Error>>,
}

impl<H: Host> Driver<H> {
    /// A driver with no window yet.
    pub(super) fn new(config: Config, host: H) -> Self {
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
        // moved off it receives none -- and a literal here would publish "one
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
                // modifiers and possibly an input method -- which is why a key
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
    /// and without it the two paths disagreed -- a game reading
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
