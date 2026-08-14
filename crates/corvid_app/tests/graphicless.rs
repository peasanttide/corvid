//! A build with no graphics stack in it, and the proof that it has none.
//!
//! The claim is about a *dependency graph* rather than about behaviour, so the
//! test is too: `cargo tree` is asked whether anything in a build of this crate
//! without `render` reaches `wgpu`, and the answer has to be nothing.
//!
//! # Why this is worth a test at all
//!
//! Because it is one line away from being false in either direction and no
//! ordinary test would notice. A `use corvid_render::...` added to a module that
//! is not feature-gated compiles fine -- the feature is on in every other build
//! in the workspace -- and quietly puts a graphics library into a dedicated
//! server's binary. The last time the two were separate they drifted back
//! together exactly that way.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    reason = "a failed assertion in a test is a failed test, which is what a test is for"
)]

use std::process::Command;

/// Whatever the test needs to say went wrong.
type Fallible = Result<(), Box<dyn std::error::Error>>;

/// Asks `cargo tree` what a build of this crate reaches.
///
/// `--no-default-features` and then exactly the features named, so that the
/// answer is about the build being asked about rather than about whatever the
/// workspace happens to unify to.
fn tree(features: &str) -> Result<String, Box<dyn std::error::Error>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut command = Command::new(cargo);
    command
        .arg("tree")
        .arg("--package")
        .arg("corvid_app")
        .arg("--no-default-features")
        .arg("--edges")
        .arg("normal");
    if !features.is_empty() {
        command.arg("--features").arg(features);
    }
    let out = command.output()?;
    if !out.status.success() {
        return Err(format!(
            "cargo tree refused: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// **The claim.** Every build of the runtime reaches a graphics library.
///
/// The runtime names `Render` in the `App`'s own bounds -- a `Game` has a
/// renderer whether or not it opens one -- so `corvid_render` is an
/// unconditional dependency and there is no build of this crate without a
/// graphics stack in it.
///
/// A feature deciding whether `corvid_render` compiled at all would only buy
/// otherwise in a workspace where *nothing whatsoever* enabled it, because
/// Cargo unifies features across one. That is a price knowingly paid rather
/// than an oversight, and this test is where it is written down.
///
/// What a headless build still costs nothing for is the **device**:
/// `Render::REAL` is false for `()`, so no adapter is requested, no surface is
/// acquired and `draw` is never called. That is a claim about a run rather than
/// about a dependency graph, and `tests/windowless.rs` is where it is made.
#[test]
fn every_build_of_the_runtime_compiles_wgpu() -> Fallible {
    let plain = tree("")?;
    assert!(
        plain.contains("wgpu"),
        "the runtime no longer reaches wgpu, which would mean `Render` had left          the `App`'s bounds",
    );
    Ok(())
}

/// And the netcode does not need one either.
///
/// The build a dedicated server is: a session, a transport, and no pictures.
#[test]
fn a_server_build_is_netcode_without_pictures() -> Fallible {
    let server = tree("net")?;
    assert!(
        server.contains("corvid_lockstep") && server.contains("corvid_net"),
        "a build with `net` does not reach the netcode",
    );
    // And it reaches wgpu, because every build of the runtime does -- see
    // `every_build_of_the_runtime_compiles_wgpu` for why that is the design
    // rather than a leak. A dedicated server is one that opens no device, not
    // one that cannot.
    assert!(server.contains("wgpu"));
    Ok(())
}

/// Asking for `render` really does add it, so the test above is measuring
/// something.
///
/// A graph assertion that passed because the feature name was misspelled, or
/// because `cargo tree` was answering about a package that does not exist,
/// would look exactly like a graph assertion that passed.
#[test]
fn asking_for_render_reaches_a_graphics_library() -> Fallible {
    let drawing = tree("render")?;
    assert!(
        drawing.contains("wgpu"),
        "a build with `render` does not reach wgpu, so the graphicless claim is \
         not about anything",
    );
    Ok(())
}
