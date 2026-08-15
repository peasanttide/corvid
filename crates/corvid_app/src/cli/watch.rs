//! The subscriber a binary installs, so this framework's own events are
//! visible.
//!
//! The seam against `entry.rs` is that this is optional and that is not: a
//! game's `main` may install a subscriber of its own, or none, and still be a
//! game.

/// Installs the subscriber that makes this framework's own events visible.
///
/// Every crate here reports through `tracing` -- which adapter was chosen, which
/// frames were dropped, what the netcode did with a late datagram -- and **not
/// one of them appears without a subscriber installed**.
///
/// It is still a binary's decision, which is why this is a function a `main`
/// calls rather than something a library does on its own: a library that
/// installed a subscriber would be a library nobody can silence. What it stops
/// is every game writing the same twelve lines to make the framework audible.
/// [`main`](crate::main) calls it; a game building its own [`App`](crate::App) calls this or does not.
///
/// `RUST_LOG` picks the level, as it does everywhere else; the default is
/// `info`, which is the level a chosen adapter and a dropped frame are reported
/// at. `RUST_LOG=corvid_net=debug` is how a link's individual datagrams become
/// visible.
///
/// Events go to **stderr**, which leaves a program's own answer alone on stdout
/// for a pipe.
///
/// Calling it twice is not an error and not this function's business: a
/// subscriber already installed stays, and a game that installed its own before
/// reaching a Corvid `main` keeps it.
pub fn watch() {
    use tracing_subscriber::{EnvFilter, fmt};

    drop(
        fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .with_target(true)
            .try_init(),
    );
}
