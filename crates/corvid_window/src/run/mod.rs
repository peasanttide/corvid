//! The event loop that owns `main`.
//!
//! Three files under this one, split by when they run: `driver.rs` is
//! everything that happens inside the loop, `convert.rs` is the stateless
//! arithmetic it turns platform numbers into Corvid values with, and this is
//! the call that hands the thread over. `error.rs` is what either half refuses
//! with.

use winit::event_loop::{ControlFlow, EventLoop};

mod convert;
mod driver;
mod error;

pub use error::{Error, Opening};

use crate::host::{Config, Host};
use driver::Driver;

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
    // first would mean a platform failure at teardown -- an X connection dropped
    // during the server sync `run_app` performs after the loop breaks, say --
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
/// The Windows one is deliberately not used -- it works there and a Windows
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
