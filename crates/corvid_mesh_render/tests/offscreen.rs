//! The whole path, on a machine with no display: fixed-point geometry in,
//! pixels out.
//!
//! Every test here goes through the same [`Renderer`] a window uses -- the same
//! acquire, the same encoder, the same submit, the same conversion out of fixed
//! point -- and differs only in where the frame lands. That is what makes these
//! tests about the renderer rather than about a second implementation of one.
//!
//! The pipeline and the shader belong to this file rather than to the crate,
//! because after the pass graph went there is no pipeline in `corvid_render`
//! at all. Bringing one is what a game does, and it is what
//! [`Graphics::new`] is: eighty lines that a game writes once and that this
//! file writes so the device path can be exercised without one.
//!
//! # When these do not run
//!
//! They need an adapter: a real GPU, or a software rasteriser such as Mesa's
//! `lavapipe`. On a machine with neither, [`Renderer::offscreen`] answers
//! `Error::NoAdapter` and each test below prints why it stopped and passes.
//! That is a deliberate hole and it is the reason `src/matrix.rs` carries its
//! own tests: the conventions a projection can get wrong are checked without a
//! device, and these check that a device does what the conventions say.
//!
//! # Why every one of them is under a deadline
//!
//! These are the only tests in the workspace that wait on something outside the
//! process. [`Renderer::read_back`] submits work and then polls the device with
//! `PollType::Wait { timeout: None }`, which has no deadline of its own, and each
//! test below opens a device of its own -- so several software-rasteriser devices
//! exist at once and each of them runs worker threads.
//!
//! That wedges, and it is not rare: with all of them rendering at once, about one
//! release run in three never came back. This binary was found spinning a core
//! with its test threads parked on futexes and a driver worker at a hundred per
//! cent, half an hour after the run that started it was over, and the run that
//! started it had been reported clean.
//!
//! So there are three things here, and each covers what the one before it does
//! not. The tests take [`RENDERING`] in turn, which is what makes the wedge rare
//! rather than routine -- one device at a time is the load this was ever tested
//! under. Each runs on a thread of its own under [`PATIENCE`], so a wedge is a
//! named failing test rather than silence. And [`impatience`] aborts the
//! process, because a wedged driver thread does not let it exit: the run below
//! reported its failures at the deadline and then sat there until something
//! killed it, which is the whole hang over again with a nicer message on it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

mod common;

use common::deadline::drawing;

use corvid_fixed::Signed32;

use common::{
    CLEARED, Direction, Extent, OctDirection, at, cube, depth_texture, drawn, opened, pixel,
};

#[test]
fn a_cube_covers_the_middle_and_leaves_the_corners_alone() {
    // Both halves are needed. The middle alone would pass on a renderer that
    // filled the frame with the tint and never looked at the geometry, and the
    // corner alone would pass on one that drew nothing.
    drawing(
        "a_cube_covers_the_middle_and_leaves_the_corners_alone",
        || {
            let Some((mut renderer, graphics)) = opened(&cube(None)) else {
                return;
            };
            let image = drawn(&mut renderer, &graphics, &[at(0.0, [0.9, 0.47, 0.16, 1.0])]);

            assert_ne!(pixel(&image, 32, 32), CLEARED, "nothing was drawn");
            assert_eq!(pixel(&image, 0, 0), CLEARED, "the whole frame was drawn");
            // And it is the tint rather than an arbitrary colour: red is the
            // largest channel of the tint and it must be the largest channel on
            // screen, which the clear colour's blue-dominant triple is not.
            let middle = pixel(&image, 32, 32);
            assert!(
                middle[0] > middle[2],
                "the cube came out {middle:?} from an orange tint",
            );
        },
    );
}

#[test]
fn an_empty_frame_leaves_the_clear_colour_everywhere() {
    // The control for the test above: with nothing recorded but the clear,
    // every pixel is what the pass cleared to. A renderer that left the
    // previous frame in place fails here rather than in a test about geometry.
    drawing("an_empty_frame_leaves_the_clear_colour_everywhere", || {
        let Some((mut renderer, graphics)) = opened(&cube(None)) else {
            return;
        };
        let image = drawn(&mut renderer, &graphics, &[]);

        for (index, chunk) in image.pixels.chunks_exact(4).enumerate() {
            assert_eq!(chunk, CLEARED, "pixel {index} is not the clear colour");
        }
    });
}

#[test]
fn the_nearer_cube_hides_the_further_one() {
    // The depth test, in the one direction that distinguishes it from having
    // none: the further cube is drawn *second*, so without depth it would paint
    // over the nearer one. The reverse order is checked too, because a depth
    // test that compared the wrong way would pass the first half alone.
    drawing("the_nearer_cube_hides_the_further_one", || {
        let Some((mut renderer, graphics)) = opened(&cube(None)) else {
            return;
        };
        let near = [0.94, 0.16, 0.16, 1.0];
        let far = [0.16, 0.94, 0.16, 1.0];

        let first = pixel(
            &drawn(&mut renderer, &graphics, &[at(0.0, near), at(30.0, far)]),
            32,
            32,
        );
        let second = pixel(
            &drawn(&mut renderer, &graphics, &[at(30.0, far), at(0.0, near)]),
            32,
            32,
        );

        assert_eq!(first, second, "the order the cubes were listed in mattered");
        assert!(
            first[0] > first[1],
            "the far cube won the depth test: {first:?}",
        );
    });
}

#[test]
fn the_normal_reaches_the_shader_and_is_decoded_there() {
    // The claim the fixed-point vertex adds: two bytes of `Snorm8x2` become a
    // direction in the shader. The same cube is drawn twice with every normal
    // replaced -- once pointing at the light and once away from it -- so the two
    // frames differ only in that pair of bytes.
    //
    // A shader that ignored the attribute would make the two equal. One that
    // decoded it to a constant would too. Only a decode that reads the pair
    // separates them, and the direction of the difference is asserted as well
    // as its existence: the face turned toward the light is the brighter one,
    // which a decoder with the sign of `w` inverted gets backwards.
    drawing("the_normal_reaches_the_shader_and_is_decoded_there", || {
        // The light in `cube.wgsl` travels along +Y and downwards, so a normal
        // pointing back along -Y catches it and one along +Y does not.
        let toward = OctDirection::encode(Direction::new(
            Signed32::ZERO,
            Signed32::from_f64(-1.0),
            Signed32::ZERO,
        ));
        let away = OctDirection::encode(Direction::new(
            Signed32::ZERO,
            Signed32::from_f64(1.0),
            Signed32::ZERO,
        ));
        assert_ne!(
            toward.to_array(),
            away.to_array(),
            "the fixture is degenerate"
        );

        let white = [1.0, 1.0, 1.0, 1.0];
        let lit = {
            let Some((mut renderer, graphics)) = opened(&cube(Some(toward))) else {
                return;
            };
            pixel(&drawn(&mut renderer, &graphics, &[at(0.0, white)]), 32, 32)
        };
        let unlit = {
            let Some((mut renderer, graphics)) = opened(&cube(Some(away))) else {
                return;
            };
            pixel(&drawn(&mut renderer, &graphics, &[at(0.0, white)]), 32, 32)
        };

        assert!(
            lit[0] > unlit[0],
            "the normal did not change the shading: lit {lit:?}, unlit {unlit:?}",
        );
    });
}

#[test]
fn resizing_changes_what_comes_back() {
    // A resize has to reach the colour texture, and the game's own depth
    // texture has to follow it, or the next frame is a validation error rather
    // than a wrong picture. The cube is still drawn, so a resize that quietly
    // stopped drawing would fail here too.
    drawing("resizing_changes_what_comes_back", || {
        let Some((mut renderer, mut graphics)) = opened(&cube(None)) else {
            return;
        };
        let smaller = Extent::new(32, 16);
        renderer.resize(smaller);
        graphics.depth = depth_texture(renderer.device(), smaller);

        let image = drawn(&mut renderer, &graphics, &[at(0.0, [0.9, 0.47, 0.16, 1.0])]);

        assert_eq!(image.size, smaller);
        assert_eq!(image.pixels.len(), 32 * 16 * 4);
        assert_ne!(pixel(&image, 16, 8), CLEARED, "the resized frame is empty");
    });
}

#[test]
fn the_same_frame_twice_is_the_same_bytes_and_survives_a_png() {
    // Two things at once, and they belong together because each is what makes
    // the other worth anything.
    //
    // The first is what pins the exact-match arm of a capture comparison: one
    // adapter drawing one frame twice produces one answer, so a byte that moved
    // between two runs on the same machine moved for a reason. It says nothing
    // about two *different* adapters, which is the whole reason a PNG golden
    // carries a tolerance.
    //
    // The second is that the encoding is lossless. A capture that quantized or
    // reordered channels on the way to a file would make every later comparison
    // a comparison of the encoder.
    drawing(
        "the_same_frame_twice_is_the_same_bytes_and_survives_a_png",
        || {
            let Some((mut renderer, graphics)) = opened(&cube(None)) else {
                return;
            };
            let orange = [0.9, 0.47, 0.16, 1.0];
            let once = drawn(&mut renderer, &graphics, &[at(0.0, orange)]);
            let twice = drawn(&mut renderer, &graphics, &[at(0.0, orange)]);
            assert_eq!(once.pixels, twice.pixels, "one adapter drew two frames");

            let encoded = once.to_png().unwrap();
            assert_eq!(&encoded[1..4], b"PNG", "that is not a PNG");
            let mut reader = png::Decoder::new(std::io::Cursor::new(&encoded))
                .read_info()
                .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut pixels).unwrap();
            pixels.truncate(info.buffer_size());
            assert_eq!(pixels, once.pixels, "the PNG is not the frame");
        },
    );
}
