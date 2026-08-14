//! Court metres to clip space, and the quads a frame is made of.
//!
//! The seam against `mod.rs` is the device: nothing here names one. This is
//! the arithmetic that turns two states and a weight into vertices, which is
//! what makes `tests/drawn.rs` able to check a picture without an adapter.

use corvid::{Extent, Factor16, Factor32, I16F16};

use crate::art::{BALL, LEFT, LINES, PIP, RIGHT, STRUCK, Vertex};
use crate::{Contact, Court, FLASH, SEATS, Table};

/// Court metres to clip space, with the court's shape preserved whatever the
/// window's is.
///
/// The court is letterboxed rather than stretched: a window twice as wide as it
/// should be gets bars at the sides, because a pong court that changes
/// proportion with the window would change what a shot across it looks like.
pub(super) struct Space {
    /// What one metre is in clip space along `x`.
    pub(super) scale_x: f32,
    /// And along `y`.
    pub(super) scale_y: f32,
}

impl Space {
    /// The mapping for this court in this target.
    ///
    /// The pixel counts become `f32` here, which is a narrowing the compiler is
    /// right to mention and is exactly right for what it is used for: a window
    /// wider than sixteen million pixels does not exist, and what this produces
    /// is a scale a rasteriser will round to a pixel anyway.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a window's width in pixels is far inside what an f32 counts exactly, and the result is a scale that ends up rounded to a pixel"
    )]
    pub(super) fn new(court: &Court, size: Extent) -> Self {
        let half_x = court.half.x().to_f32().max(f32::EPSILON);
        let half_y = court.half.y().to_f32().max(f32::EPSILON);
        // A margin, so the court's own edge is visible rather than flush with
        // the window's.
        let fill = 0.94;
        let width = size.width.max(1) as f32;
        let height = size.height.max(1) as f32;
        let per_metre = (fill / half_x * 0.5 * width).min(fill / half_y * 0.5 * height);
        Self {
            scale_x: per_metre / (0.5 * width),
            scale_y: per_metre / (0.5 * height),
        }
    }

    /// One rectangle, given its centre and half-extents in court metres.
    fn quad(&self, into: &mut Vec<Vertex>, at: [f32; 2], half: [f32; 2], tint: [f32; 4]) {
        let (cx, cy) = (at[0] * self.scale_x, at[1] * self.scale_y);
        let (hx, hy) = (half[0] * self.scale_x, half[1] * self.scale_y);
        let corners = [
            [cx - hx, cy - hy],
            [cx + hx, cy - hy],
            [cx + hx, cy + hy],
            [cx - hx, cy - hy],
            [cx + hx, cy + hy],
            [cx - hx, cy + hy],
        ];
        into.extend(corners.map(|at| Vertex { at, tint }));
    }
}

/// Every rectangle in the picture, in the order they are drawn.
/// The two states, the flash and the weight: everything `paint` reads.
pub(super) struct Painted {
    pub(super) previous: Table,
    pub(super) current: Table,
    pub(super) since_goal: f32,
    pub(super) alpha: corvid::Factor16,
}

pub(super) fn paint(into: &mut Vec<Vertex>, space: &Space, frame: &Painted, court: &Court) {
    let half_x = court.half.x().to_f32();
    let half_y = court.half.y().to_f32();
    let edge = 0.06;

    // The court: two side lines and the net down the middle. Flashed after a
    // goal, which is the one thing the view is for.
    let glow = 1.0 - (frame.since_goal / FLASH).clamp(0.0, 1.0);
    let lines = [
        LINES[0] + glow * 0.5,
        LINES[1] + glow * 0.45,
        LINES[2] + glow * 0.4,
        1.0,
    ];
    space.quad(into, [0.0, half_y], [half_x, edge], lines);
    space.quad(into, [0.0, -half_y], [half_x, edge], lines);
    space.quad(into, [0.0, 0.0], [edge * 0.5, half_y], lines);

    // The paddles, at the ends they defend. Interpolated between the two states
    // the display sits between, which is what keeps a thirty-hertz simulation
    // from looking like one.
    let alpha = frame.alpha.to_factor32();
    for seat in 0..SEATS {
        let (Some(before), Some(now)) = (
            frame.previous.paddles.get(seat),
            frame.current.paddles.get(seat),
        ) else {
            continue;
        };
        let at = before.at.lerp(now.at, alpha).to_f32();
        let tint = if seat == 0 { LEFT } else { RIGHT };
        space.quad(
            into,
            // `centre`, not `face`: the face is the plane the ball bounces off,
            // which is this rectangle's court-facing edge rather than a line
            // through the middle of it.
            [court.centre(seat).to_f32(), at],
            [court.paddle.x().to_f32(), court.paddle.y().to_f32()],
            tint,
        );
    }

    // The ball, which is not drawn at all while it is waiting to be served --
    // the state says so, and a ball parked at the centre for a second would
    // read as a ball that had stopped working.
    if frame.current.serve == 0 {
        let at = shown(frame, alpha);
        let struck = matches!(frame.current.contact, Some(Contact::Paddle { .. }));
        let tint = if struck { STRUCK } else { BALL };
        let size = court.ball.to_f32();
        space.quad(into, at, [size, size], tint);
    }

    // The score, as a pip per point along the top of each half.
    for seat in 0..SEATS {
        let Some(score) = frame.current.scores.get(seat) else {
            continue;
        };
        let side = if seat == 0 { -1.0 } else { 1.0 };
        let tint = if seat == 0 { LEFT } else { RIGHT };
        let pip = half_y * PIP;
        for point in 0..(*score).min(16) {
            let along = side * (f32::from(point) * pip).mul_add(-3.0, half_x * 0.5);
            space.quad(into, [along, half_y * 0.8], [pip, pip], tint);
        }
    }
}

/// Where the ball is for the frame being displayed.
///
/// The interpolation belongs to the client and never to the simulation, which
/// is the same rule `examples/hello` states: `weight` is exact at both ends, so
/// this is `previous` at zero and `current` at one, bit for bit.
const fn shown(frame: &Painted, alpha: Factor32) -> [f32; 2] {
    let (before, now) = (frame.previous.ball.at, frame.current.ball.at);
    [
        before.x().lerp(now.x(), alpha).to_f32(),
        before.y().lerp(now.y(), alpha).to_f32(),
    ]
}

/// The ball's position at a frame, for whoever wants it in court metres.
///
/// Public because a test asserts that the two ends of the interpolation are the
/// two states exactly, which is the obligation `Render::draw` is held to.
#[must_use]
pub fn ball_at(previous: &Table, current: &Table, alpha: Factor16) -> [I16F16; 2] {
    let frame = Painted {
        previous: previous.clone(),
        current: current.clone(),
        since_goal: 0.0,
        alpha,
    };
    let alpha = frame.alpha.to_factor32();
    let (before, now) = (frame.previous.ball.at, frame.current.ball.at);
    [
        before.x().lerp(now.x(), alpha),
        before.y().lerp(now.y(), alpha),
    ]
}

/// What the court looks like with nothing on it, for a caller that wants the
/// state a picture is drawn from without a device to draw it on.
#[must_use]
pub const fn empty() -> Table {
    Table {
        ball: crate::table::Ball {
            at: corvid::FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
            velocity: corvid::FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
        },
        paddles: [crate::table::Paddle { at: I16F16::ZERO }; SEATS],
        scores: [0; SEATS],
        serve: 0,
        towards: true,
        contact: None,
        now: corvid::Tick::ZERO,
        over: None,
    }
}
