//! The one-`wgpu` rule, in its mechanical form.
//!
//! There is one `wgpu` in this workspace's graph and one `raw-window-handle`
//! with it, which matters because a surface cannot be created from a window
//! handle of a different version and the failure is a runtime one that looks
//! like a driver problem.
//!
//! **What enforces that is the workspace pin, not a re-export.** `corvid` is
//! the workspace's one facade, so this crate names `wgpu` itself, and what
//! keeps the version single is `wgpu = { workspace = true }` resolving to the
//! one entry in the root manifest. So the assertion is not "do not name it" but
//! "do not name a *version* of it", and a `wgpu = "31"` added here on a busy
//! afternoon is exactly what this catches.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

/// This crate's own manifest, read at build time so the test needs no working
/// directory to find it.
const MANIFEST: &str = include_str!("../Cargo.toml");

/// Every `wgpu` line in this manifest, comments excluded.
fn wgpu_lines() -> Vec<&'static str> {
    MANIFEST
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.starts_with("wgpu"))
        .collect()
}

#[test]
fn wgpu_is_taken_from_the_workspace_rather_than_pinned_here() {
    let named = wgpu_lines();
    assert!(
        !named.is_empty(),
        "this crate creates textures on a device, so it names the device"
    );
    for line in &named {
        assert!(
            line.contains("workspace = true"),
            "a version pinned here is a second wgpu in the graph: {line}"
        );
        assert!(
            !line.contains("version"),
            "the version belongs in the root manifest and nowhere else: {line}"
        );
    }
}

#[test]
fn it_names_the_plan_crate_it_is_the_other_half_of() {
    assert!(
        MANIFEST
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with("corvid_image ")),
        "the plan has to come from somewhere, and this is where"
    );
    // And the constant it shares with that crate is one number rather than two,
    // which is what makes the shared `corvid_image` observable rather than
    // merely asserted.
    const { assert!(corvid_image::MAX_NUM_MAPS <= corvid_image_render::MAX_SOURCES) };
}
