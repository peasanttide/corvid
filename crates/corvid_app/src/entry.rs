//! The one entry point, and the reason there is only one.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use corvid_hash::digest;
use corvid_replay::Opens;

use crate::{App, Argument, Arguments, Error, Outcome, Result};

/// How big a run with a device and no window draws.
///
/// A run like that has nowhere to show a frame, so the only thing the size
/// decides is what a `--capture` writes. Seven hundred and twenty rows is what
/// a picture is expected to be when nobody said; a run that wants another size
/// builds its own [`App`] and calls
/// [`offscreen`](App::offscreen).
#[cfg(not(feature = "window"))]
#[cfg(feature = "render")]
const OFFSCREEN: corvid_render::Extent = corvid_render::Extent::new(1280, 720);

/// What an [`Error::Wrote`] about the usage names, since stdout is not a file
/// and [`Error::Wrote`] carries a path.
const STDOUT: &str = "<stdout>";

/// Plays the game the process was started to play.
///
/// **This is the whole of a Corvid `main`, and there is no second spelling.**
///
/// ```no_run
/// # use std::sync::Arc;
/// # use corvid_behavior::{Level, State};
/// # use corvid_files::{Malformed, Source};
/// # use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
/// # use corvid_time::Tick;
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Nowhere;
/// # impl Level for Nowhere {
/// #     type Reference = String;
/// #     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
/// # }
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// # struct Server;
/// # impl State for Server {
/// #     const NAME: &'static str = "server";
/// #     type Level = Nowhere;
/// #     type Rules = ();
/// #     type Action = ();
/// # }
/// # impl corvid_replay::Opens for Server {
/// #     fn opening() -> corvid_replay::Opening<Self> { unimplemented!() }
/// # }
/// // A game that draws nothing says nothing at all: `App`'s renderer defaults
/// // to `()`, which opens no device and is never asked to draw. There is no
/// // line to write, which is the shortest way of drawing nothing there is.
/// fn main() -> corvid_app::Result {
///     corvid_app::main::<Server>()
/// }
/// ```
///
/// A window, a headless run, a capture, a replay and a save slot are all the
/// same program: this reads the process's arguments and decides. **A game never
/// asks for determinism**, because a game that had to call `.headless()` would
/// have a mode that is deterministic and a mode that is not, and only one of
/// them would be tested. [`Arguments`] is the list of what an operator may say.
///
/// # Which backend this picks
///
/// `--headless` is no window, no adapter and no audio device. Without it, a
/// build with the `window` feature opens a window; a build without one draws
/// into a texture if there is a `--capture` to write the pictures into, and
/// plays headless otherwise, because a device drawing frames nothing will look
/// at or keep is a device doing nothing. Every build can open a device, so
/// `--headless` means the same thing in all of them — which is what lets a
/// script pass it without knowing which build it is talking to.
///
/// # The bound, and the game that has nothing to draw
///
/// `S: State`, which is `G: Render`: the client-local half is one chain of
/// traits over one marker, so a game that reaches this has a `setup` and a
/// `draw` whether it draws anything or not. There is no configuration in which
/// the bound is weaker, and there used to be — a trait reconciling `Render`
/// with `Present` under opposite `cfg`s, which existed because a `wgpu` type
/// could not be named a crate lower down. It can now.
///
/// `type Graphics = ();` is the one line that satisfies the bound for a game
/// with nothing to draw: a dedicated server, a determinism check, or a game
/// that has not drawn anything yet writes that line and writes no `wgpu`. The
/// view is declared on [`Render`](corvid_render::Render) rather than beside
/// it, because the macro supplies the whole implementation and only the game
/// knows what its view is.
///
/// # `--help` is not a failure
///
/// An operator who asked for the usage got what they asked for. This writes it
/// to **stdout** and answers `Ok(())`, so the process exits zero and a shell
/// script does not have to special-case it. [`Arguments::parse`] still reports
/// it as [`Argument::Help`], because that function may not print; this is the
/// one place in the crate that may, and it is a `write!` to
/// [`io::stdout`](std::io::stdout) rather than a `println!` for the reason
/// [`Arguments::USAGE`] gives. Every other command line that could not be acted
/// on goes to stderr through the `Err` this hands back, and exits non-zero.
///
/// # What it does with the exit code
///
/// A [`quit`](corvid_behavior::Command::quit) names a status, and a status that
/// is not [`SUCCESS`](corvid_behavior::ExitCode::SUCCESS) leaves the process
/// with that number rather than with the `1` that any `Err` from a `main`
/// collapses to. Everything a run writes down has been written by then: a
/// capture is closed and a save is on disk before the run hands its outcome
/// back.
///
/// # Errors
///
/// [`Error::Argument`] for a command line that could not be acted on,
/// [`Error::Wrote`] if the usage was asked for and stdout would not take it,
/// and then whatever [`App::run`] reports.
pub fn main<S: corvid_behavior::State + Opens>() -> Result {
    let Some(arguments) = command_line()? else {
        return Ok(());
    };
    // Whether the operator asked for a run with no devices, which is the one
    // thing that decides whether the ending tick and digest go to stdout.
    let headless = arguments.headless;
    // The declaration, and then the table written against it. Neither of these
    // was here once, and a game played through this `main` therefore ran with
    // an empty declaration — which binds no key and no axis and answers
    // `RELEASED` to every query for the length of the run. `Present::SETS` is
    // what closed it, and it is required rather than defaulted so that the
    // same hole cannot be dug again.
    let app = App::<S>::new()
        .opening(S::opening())
        .input(corvid_input::Input::new(&[]));
    // The table is a windowed run's business alone: `App::bindings` holds a
    // `corvid_window::Bindings` and a run with no window reads no device.
    #[cfg(feature = "window")]
    let app = app;
    let app = if arguments.headless {
        app
    } else {
        #[cfg(feature = "window")]
        {
            app.window()
        }
        // No window, and a capture asked for: draw into a texture instead, so
        // that a build machine still gets a picture. A build with no graphics
        // stack at all has no third case — it writes the audio, the trace and
        // the session, and says nothing about pixels.
        #[cfg(all(not(feature = "window"), feature = "render"))]
        if arguments.capture.is_some() {
            app.offscreen(OFFSCREEN)
        } else {
            app
        }
        #[cfg(all(not(feature = "window"), not(feature = "render")))]
        {
            app
        }
    };
    finish(&app.arguments(arguments).run()?, headless);
    Ok(())
}

/// Reads the process's arguments, answering a request for the usage on the way.
///
/// [`None`] is "the usage was asked for and has been written, and there is no
/// game left to play" — which is a success, and is why this is an `Option`
/// inside the `Ok` rather than a third error variant.
///
/// # Errors
///
/// [`Error::Argument`] for a command line that could not be acted on, and
/// [`Error::Wrote`] if stdout would not take the usage.
fn command_line() -> Result<Option<Arguments>> {
    match Arguments::from_env() {
        Ok(arguments) => Ok(Some(arguments)),
        Err(Argument::Help) => {
            // `write!` on the handle rather than `println!`, and the difference
            // is not cosmetic: the workspace denies the printing macros because
            // a library that reaches for the process's streams by macro is one
            // nobody can redirect or test. This is a `Write` implementation
            // being written to, which a caller can substitute and a test can
            // read back — `Arguments::USAGE` is the text, and it is public for
            // exactly that reason.
            let mut out = io::stdout();
            writeln!(out, "{}", Arguments::USAGE)
                .and_then(|()| out.flush())
                .map_err(|why| Error::Wrote {
                    path: PathBuf::from(STDOUT),
                    why,
                })?;
            Ok(None)
        }
        Err(why) => Err(Error::Argument(why)),
    }
}

/// Reports where the run got to, and leaves the process with the status a
/// [`quit`](corvid_behavior::Command::quit) named.
///
/// The report is a `tracing` event, so a game that wants it structured installs
/// a subscriber. A **headless** run also writes the ending tick and digest to
/// stdout, because that is what an operator asked for by passing `--headless`
/// and a `main` of three lines has nowhere to install a subscriber. A windowed
/// run prints nothing: the digest is not what somebody watching a window came
/// for.
fn finish<S: corvid_behavior::State>(outcome: &Outcome<S>, headless: bool) {
    if headless {
        // A refused write to stdout is not worth ending a run that already
        // succeeded over — the tracing event below carries the same values.
        let mut out = io::stdout();
        let _ = writeln!(
            out,
            "tick {} mark {}",
            outcome.session.last(),
            digest(&outcome.state)
        );
        let _ = out.flush();
    }
    tracing::info!(
        name: "corvid_app.finished",
        tick = %outcome.session.last(),
        mark = %digest(&outcome.state),
        requests = outcome.requests.len(),
        "the run ended",
    );
    if outcome.exit != corvid_behavior::ExitCode::SUCCESS {
        // The run is over and everything it writes down is written: `App::run`
        // closes a capture before it hands an outcome back. What is left to do
        // is hand the operating system the number the game asked for, and a
        // `main` returning `Result` cannot — every `Err` from one is status 1.
        std::process::exit(i32::from(outcome.exit.0));
    }
}
