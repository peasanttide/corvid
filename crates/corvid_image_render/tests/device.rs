//! The whole path on a real adapter: a plan in, tiles on a texture, and a
//! frame that read them back through this crate's own WGSL.
//!
//! This is the only test that can catch the failure this crate exists to
//! prevent -- the shader and the lookup table disagreeing about where a texel
//! is -- because the only way to check a shader's addressing is to run it and
//! compare it against the same arithmetic written somewhere else.
//! [`TileTable::resolve`] is that somewhere else, and every assertion below
//! compares one against the other to the texel.
//!
//! # When this does not run
//!
//! It needs an adapter: a real GPU, or a software rasteriser such as Mesa's
//! `lavapipe`. On a machine with neither, [`Renderer::offscreen`] answers
//! `Error::NoAdapter`, this prints why it stopped and passes. That is a
//! deliberate hole and it is why `tests/geometry.rs` exists: everything that can
//! be checked without a device is checked there, and this checks that a device
//! does what that arithmetic says.
//!
//! # Why it is one test
//!
//! Every device test in this workspace opens a device of its own, and several
//! software-rasteriser devices at once is the condition under which
//! `corvid_mesh_render`'s offscreen tests were found wedged. There is one
//! `#[test]` here and it opens one device, which is the load a renderer is
//! actually used under.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "every float here is a pixel index turned into the uv a shader was handed"
)]
#![allow(
    clippy::print_stderr,
    reason = "a test that is skipped has to say so where a person running the suite will see it, and the workspace's answer everywhere else -- a tracing event -- needs a subscriber that a test harness does not install"
)]

mod common;

use common::{PLATE, Probing, SEAL, SIZE, painted, pixel, streamed};

use corvid_image::TileKey;
use corvid_image_render::CacheError;
use corvid_render::Renderer;

#[test]
fn the_shader_reads_the_table_the_way_the_planner_wrote_it() {
    let mut renderer = match Renderer::offscreen(SIZE) {
        Ok(renderer) => renderer,
        Err(why) => {
            eprintln!("skipped: this machine has no adapter to render with ({why})");
            return;
        }
    };
    let (plan, mut cache) = streamed(&mut renderer);
    let probing = Probing::new(renderer.device(), renderer.format(), &cache);
    let table = plan.table();

    // Every pixel of the frame is one uv, and the shader's answer for it has to
    // be `TileTable::resolve`'s answer for it. This is the assertion the crate
    // exists for: sixty-four squared samples of two independent implementations
    // of one addressing scheme.
    let frame = probing.frame(&mut renderer, &cache, PLATE, 0, 0.0);
    let mut resolved = 0u32;
    for y in 0..SIZE.height {
        for x in 0..SIZE.width {
            let uv = [
                (x as f32 + 0.5) / SIZE.width as f32,
                (y as f32 + 0.5) / SIZE.height as f32,
            ];
            let got = pixel(&frame, x, y);
            match table.resolve(PLATE, uv) {
                Some(sample) => {
                    resolved += 1;
                    assert_eq!(
                        got,
                        [
                            u8::try_from(sample.texel[0]).unwrap(),
                            u8::try_from(sample.texel[1]).unwrap(),
                            sample.level,
                            255,
                        ],
                        "at pixel ({x}, {y}), uv {uv:?}",
                    );
                }
                None => assert_eq!(got[3], 0, "the shader found a tile the table did not"),
            }
        }
    }
    assert_eq!(
        resolved,
        SIZE.width * SIZE.height,
        "the budget was meant to leave the whole plate resident",
    );

    // And the tile in the slot is the one the plan named, which the frame above
    // could not tell: `mode 0` never touches the atlas.
    let frame = probing.frame(&mut renderer, &cache, PLATE, 1, 0.0);
    for y in 0..SIZE.height {
        for x in 0..SIZE.width {
            let uv = [
                (x as f32 + 0.5) / SIZE.width as f32,
                (y as f32 + 0.5) / SIZE.height as f32,
            ];
            let sample = table.resolve(PLATE, uv).expect("everything is resident");
            let key = cache
                .holds(sample.slot)
                .expect("a slot the table names holds a tile");
            assert_eq!(
                pixel(&frame, x, y),
                [
                    u8::try_from(key.x).unwrap(),
                    u8::try_from(key.y).unwrap(),
                    key.level,
                    255,
                ],
                "at pixel ({x}, {y})",
            );
        }
    }

    // The mip chain: a tile of one colour reduces to that colour, so reading the
    // top of a tile's chain is reading the tile. An unbuilt chain is a
    // zero-initialised level, which is black, and that is what this catches.
    let sharp = probing.frame(&mut renderer, &cache, SEAL, 1, 0.0);
    let coarse = probing.frame(
        &mut renderer,
        &cache,
        SEAL,
        2,
        (cache.atlas().mip_levels() - 1) as f32,
    );
    for y in 0..SIZE.height {
        for x in 0..SIZE.width {
            // Compared a pixel at a time rather than as two buffers: an unbuilt
            // chain fails on every pixel of the frame, and a failure that prints
            // sixteen thousand bytes says less than one that prints four.
            assert_eq!(
                pixel(&coarse, x, y),
                pixel(&sharp, x, y),
                "the top of a uniform tile's mip chain is not the tile, at ({x}, {y})",
            );
        }
    }
    assert!(
        sharp.pixels.iter().any(|byte| *byte != 0),
        "the seal drew as nothing at all, so the comparison above proved nothing",
    );

    // An upload to a slot whose eviction was never performed is refused rather
    // than overwriting a tile the table still points at.
    let held = plan.uploads().first().expect("something was uploaded");
    let intruder = corvid_image::Upload {
        key: TileKey::new(SEAL, 7, 0, 0),
        slot: held.slot,
        priority: held.priority,
    };
    assert_eq!(
        cache.upload(renderer.queue(), &intruder, &painted(intruder.key, 16, 16)),
        Err(CacheError::SlotOccupied {
            slot: held.slot,
            held: held.key,
        }),
    );
}
