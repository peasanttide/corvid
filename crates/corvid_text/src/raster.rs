//! A glyph outline, turned into a bitmap of how covered each pixel is.

use crate::Font;
use crate::font::narrow;
use crate::scan::{Point, Scan};
use alloc::vec::Vec;
use corvid_fixed::I16F16;
use corvid_float::{ceil, floor, recip, sqrt};
use corvid_ui::GlyphId;
use ttf_parser::OutlineBuilder;

/// How much of each pixel a glyph covers, zero to full.
///
/// One byte to the pixel and no colour anywhere: colour is the caller's, and a
/// hand-coloured plate wants the same coverage tinted differently in two
/// passes. The bitmap is exactly as large as the glyph's ink, so a full stop
/// costs four pixels rather than an em square of nothing.
///
/// ```
/// use corvid_text::Coverage;
///
/// // A caller that rasterises elsewhere can still pack the result: two pixels,
/// // sitting one to the right of the pen and two above the baseline.
/// let dot = Coverage::new(2, 1, 1, -2, vec![0x40, 0xff]).unwrap();
/// assert_eq!(dot.at(1, 0), 0xff);
/// assert_eq!(dot.at(9, 9), 0, "outside the bitmap is not covered");
/// assert!(Coverage::new(2, 2, 0, 0, vec![0]).is_none(), "four pixels, one byte");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Coverage {
    /// Pixels across.
    width: u32,
    /// Pixels down.
    height: u32,
    /// Where the left edge sits, right of the pen.
    left: i32,
    /// Where the top edge sits, down from the baseline. Normally negative,
    /// because a glyph sits above the line it is on.
    top: i32,
    /// Row-major coverage, `width * height` of it.
    pixels: Vec<u8>,
}

impl Coverage {
    /// Nothing drawn: a space, or a glyph whose outline is empty.
    pub const BLANK: Self = Self {
        width: 0,
        height: 0,
        left: 0,
        top: 0,
        pixels: Vec::new(),
    };

    /// A bitmap somebody else produced.
    ///
    /// `None` when `pixels` is not exactly `width * height` bytes, which is the
    /// one invariant every method below relies on.
    #[must_use]
    pub fn new(width: u32, height: u32, left: i32, top: i32, pixels: Vec<u8>) -> Option<Self> {
        let expected = usize::try_from(width)
            .ok()?
            .checked_mul(height.try_into().ok()?)?;
        (pixels.len() == expected).then_some(Self {
            width,
            height,
            left,
            top,
            pixels,
        })
    }

    /// Pixels across.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pixels down.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Where the left edge sits, right of the pen.
    #[must_use]
    pub const fn left(&self) -> i32 {
        self.left
    }

    /// Where the top edge sits, down from the baseline, normally negative.
    #[must_use]
    pub const fn top(&self) -> i32 {
        self.top
    }

    /// The coverage, row-major.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Whether there is no ink at all.
    #[must_use]
    pub const fn is_blank(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// How covered one pixel is, zero outside the bitmap.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let index = usize::try_from(y)
            .ok()
            .and_then(|y| y.checked_mul(usize::try_from(self.width).ok()?))
            .and_then(|row| row.checked_add(usize::try_from(x).ok()?));
        index
            .and_then(|index| self.pixels.get(index))
            .copied()
            .unwrap_or(0)
    }
}

impl Font<'_> {
    /// `glyph`, drawn at `size` pixels to the em.
    ///
    /// [`Coverage::BLANK`] for a space, for a glyph the face does not hold, and
    /// for a size of zero or less. Antialiased by area rather than by sampling,
    /// so a hairline serif is grey rather than absent.
    ///
    /// This is the crate's one float seam, and it is deliberately downstream of
    /// every position: where the glyph *goes* was decided in fixed point by
    /// [`shape`](crate::shape), and what it *looks like* is decided here.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the pixel box is the outline's bounding box after `floor` and `ceil`, so each bound is already integral, and the subtraction that reaches `u32` is bounded below by zero because a ceiling is never under its floor"
    )]
    pub fn rasterize(&self, glyph: GlyphId, size: I16F16) -> Coverage {
        let scale = size.to_f32() * recip(f32::from(self.units_per_em()));
        if scale <= 0.0 || !scale.is_finite() {
            return Coverage::BLANK;
        }
        let Some(id) = narrow(glyph) else {
            return Coverage::BLANK;
        };
        // The builder has to run before the box is known: `outline_glyph`
        // answers the bounding box, and the segments have to be transformed
        // into that box. So it runs twice -- once to find the box, once to
        // draw -- rather than buffering every segment of every glyph.
        let mut measure = Measure;
        let Some(bounds) = self.face().outline_glyph(id, &mut measure) else {
            return Coverage::BLANK;
        };
        let left = floor(f32::from(bounds.x_min) * scale);
        let right = ceil(f32::from(bounds.x_max) * scale);
        let top = floor(f32::from(bounds.y_max) * -scale);
        let bottom = ceil(f32::from(bounds.y_min) * -scale);
        let (width, height) = ((right - left) as u32, (bottom - top) as u32);
        if width == 0 || height == 0 {
            return Coverage::BLANK;
        }
        let mut scan = Scan::new(width, height);
        let mut draw = Draw {
            scan: &mut scan,
            scale,
            left,
            top,
            start: (0.0, 0.0),
            pen: (0.0, 0.0),
        };
        if self.face().outline_glyph(id, &mut draw).is_none() {
            return Coverage::BLANK;
        }
        Coverage {
            width,
            height,
            left: left as i32,
            top: top as i32,
            pixels: scan.finish(),
        }
    }
}

/// An outline sink that draws nothing, so that `outline_glyph` can be asked
/// only for the bounding box it returns.
struct Measure;

impl OutlineBuilder for Measure {
    fn move_to(&mut self, _x: f32, _y: f32) {}
    fn line_to(&mut self, _x: f32, _y: f32) {}
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {}
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {}
    fn close(&mut self) {}
}

/// An outline sink that flattens curves and hands the segments to a [`Scan`].
struct Draw<'a> {
    /// Where the segments go.
    scan: &'a mut Scan,
    /// Pixels to the font unit.
    scale: f32,
    /// The left edge of the pixel box, so it can be subtracted off.
    left: f32,
    /// The top edge of the pixel box, in y-down coordinates.
    top: f32,
    /// Where the current contour began, for `close`.
    start: Point,
    /// Where the last segment ended.
    pen: Point,
}

impl Draw<'_> {
    /// A point of the em grid, in the glyph's pixel box.
    ///
    /// The y axis turns over here and nowhere else: a face measures up from the
    /// baseline and a bitmap counts down from its top row.
    fn point(&self, x: f32, y: f32) -> Point {
        (x * self.scale - self.left, -y * self.scale - self.top)
    }

    /// How many straight pieces a curve whose second difference is `deviation`
    /// needs before the difference stops showing.
    ///
    /// One piece per half-pixel of sag, which is the tolerance the eye stops
    /// reading at one byte of coverage, and never more than sixty-four, so that
    /// a pathological control point costs bounded work rather than a hang.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the fourth root of a squared deviation, floored, and clamped into 1..=64 on the same line"
    )]
    fn steps(deviation: f32) -> u32 {
        let pieces = floor(sqrt(sqrt(3.0 * deviation))) as u32;
        pieces.clamp(1, 64)
    }
}

/// One step along a straight line between two points.
fn lerp(t: f32, from: Point, to: Point) -> Point {
    (from.0 + t * (to.0 - from.0), from.1 + t * (to.1 - from.1))
}

/// How far a control point sits off the chord, squared.
fn deviation(from: Point, control: Point, to: Point) -> f32 {
    let across = from.0 - 2.0 * control.0 + to.0;
    let down = from.1 - 2.0 * control.1 + to.1;
    across * across + down * down
}

impl OutlineBuilder for Draw<'_> {
    fn move_to(&mut self, to_x: f32, to_y: f32) {
        self.start = self.point(to_x, to_y);
        self.pen = self.start;
    }

    fn line_to(&mut self, to_x: f32, to_y: f32) {
        let to = self.point(to_x, to_y);
        self.scan.line(self.pen, to);
        self.pen = to;
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "the step count is clamped into 1..=64 before it reaches a float"
    )]
    fn quad_to(&mut self, x1: f32, y1: f32, to_x: f32, to_y: f32) {
        let control = self.point(x1, y1);
        let to = self.point(to_x, to_y);
        let from = self.pen;
        let steps = Draw::steps(deviation(from, control, to));
        let step = recip(steps as f32);
        for piece in 1..steps {
            let along = piece as f32 * step;
            let at = lerp(along, lerp(along, from, control), lerp(along, control, to));
            self.scan.line(self.pen, at);
            self.pen = at;
        }
        self.scan.line(self.pen, to);
        self.pen = to;
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "the step count is clamped into 1..=64 before it reaches a float"
    )]
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, to_x: f32, to_y: f32) {
        let first = self.point(x1, y1);
        let second = self.point(x2, y2);
        let to = self.point(to_x, to_y);
        let from = self.pen;
        // A cubic sags at both ends; the worse of its two halves is what the
        // step count has to answer, and the factor of nine is the ratio between
        // a cubic's second difference and a quadratic's.
        let sag = deviation(from, first, second).max(deviation(first, second, to));
        let steps = Draw::steps(9.0 * sag);
        let step = recip(steps as f32);
        for piece in 1..steps {
            let along = piece as f32 * step;
            let head = lerp(along, from, first);
            let waist = lerp(along, first, second);
            let tail = lerp(along, second, to);
            let at = lerp(along, lerp(along, head, waist), lerp(along, waist, tail));
            self.scan.line(self.pen, at);
            self.pen = at;
        }
        self.scan.line(self.pen, to);
        self.pen = to;
    }

    fn close(&mut self) {
        self.scan.line(self.pen, self.start);
        self.pen = self.start;
    }
}
