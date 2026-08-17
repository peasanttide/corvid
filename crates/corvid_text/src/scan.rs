//! Turning line segments into coverage.
//!
//! The algorithm is the signed-area accumulator: a segment does not paint
//! pixels, it deposits the *change* in coverage at each pixel it crosses, and a
//! running sum over the buffer turns those deltas back into coverage. An upward
//! edge deposits positive and a downward edge negative, so a closed contour
//! sums to nothing outside itself without anybody testing insideness, and two
//! overlapping contours do not double-darken where they meet.
//!
//! This is the one part of the crate that is floating point, and it is the part
//! that should be: coverage is a number a sampler reads, two machines are
//! allowed to disagree about it by a step of 1/255, and nothing downstream of
//! it feeds a layout.

use alloc::vec;
use alloc::vec::Vec;
use corvid_float::{abs, ceil, floor, recip};

/// A point in the glyph's own pixel box, y down from the top of it.
pub(crate) type Point = (f32, f32);

/// The accumulation buffer a glyph is drawn into.
pub(crate) struct Scan {
    /// Pixels across, which is what comes out.
    width: usize,
    /// Pixels across the buffer, which is two more: the algorithm writes one
    /// column past the rightmost pixel a segment touches, and a segment may
    /// touch the last one.
    stride: usize,
    /// Rows.
    height: usize,
    /// The deltas, row-major.
    area: Vec<f32>,
}

impl Scan {
    /// A buffer for a glyph this many pixels across and down.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let width = usize::try_from(width).unwrap_or(usize::MAX);
        let height = usize::try_from(height).unwrap_or(usize::MAX);
        let stride = width.saturating_add(2);
        Self {
            width,
            stride,
            height,
            area: vec![0.0; stride.saturating_mul(height)],
        }
    }

    /// Deposit the coverage change of the segment from `from` to `to`.
    ///
    /// Horizontal segments are skipped: they cross no scanline, so they change
    /// coverage nowhere, and dividing by their height would be dividing by
    /// zero.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "a row index and a column index are both bounded by the glyph's own pixel box, which was computed from the outline's bounding box before this buffer was allocated; every column is clamped into the row by `deposit` regardless"
    )]
    pub(crate) fn line(&mut self, from: Point, to: Point) {
        let (direction, top, bottom) = if from.1 < to.1 {
            (1.0, from, to)
        } else {
            (-1.0, to, from)
        };
        let span = bottom.1 - top.1;
        if span <= f32::EPSILON {
            return;
        }
        let slope = (bottom.0 - top.0) * recip(span);
        let first = top.1.max(0.0);
        let last = bottom.1.min(self.height as f32);
        if last <= first {
            return;
        }
        let mut row = floor(first) as usize;
        while row < self.height && (row as f32) < last {
            let enter = first.max(row as f32);
            let leave = last.min((row + 1) as f32);
            let height = leave - enter;
            if height > 0.0 {
                let (a, b) = (
                    top.0 + (enter - top.1) * slope,
                    top.0 + (leave - top.1) * slope,
                );
                self.trapezoid(row, a, b, height * direction);
            }
            row += 1;
        }
    }

    /// One scanline's worth of one segment: it enters the row at `a`, leaves at
    /// `b`, and carries `weight` of coverage change spread across the columns
    /// between them in proportion to the area it cuts off each one.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "the columns are bounded by the glyph's pixel box and `deposit` clamps regardless; the precision loss is a column count reaching a float, and a glyph is never thousands of pixels wide"
    )]
    fn trapezoid(&mut self, row: usize, a: f32, b: f32, weight: f32) {
        let (left, right) = if a < b { (a, b) } else { (b, a) };
        let left_floor = floor(left);
        let right_ceil = ceil(right);
        let (first, last) = (left_floor as i32, right_ceil as i32);
        if last <= first + 1 {
            // The segment stays inside one pixel column: the area to its right
            // is the mean of the two crossings, and the rest belongs to the
            // column after it.
            let mean = 0.5 * (a + b) - left_floor;
            self.deposit(row, first, weight - weight * mean);
            self.deposit(row, first + 1, weight * mean);
            return;
        }
        let slice = recip(right - left);
        let entry = left - left_floor;
        let head = 0.5 * slice * (1.0 - entry) * (1.0 - entry);
        let exit = right - right_ceil + 1.0;
        let tail = 0.5 * slice * exit * exit;
        self.deposit(row, first, weight * head);
        if last == first + 2 {
            self.deposit(row, first + 1, weight * (1.0 - head - tail));
        } else {
            let second = slice * (1.5 - entry);
            self.deposit(row, first + 1, weight * (second - head));
            for column in first + 2..last - 1 {
                self.deposit(row, column, weight * slice);
            }
            let before_last = second + (last - first - 3) as f32 * slice;
            self.deposit(row, last - 1, weight * (1.0 - before_last - tail));
        }
        self.deposit(row, last, weight * tail);
    }

    /// Add `value` to one cell, clamping the column into the row.
    ///
    /// Clamping rather than discarding: the deltas of one segment in one row
    /// have to sum to its weight or the running sum leaks into the rest of the
    /// glyph, so a column that rounded outside the box lands on the edge
    /// instead of vanishing.
    fn deposit(&mut self, row: usize, column: i32, value: f32) {
        // A negative column is left of the box and clamps to its first pixel,
        // which `try_from` says as `unwrap_or(0)`.
        let column = usize::try_from(column)
            .unwrap_or(0)
            .min(self.stride.saturating_sub(1));
        if let Some(cell) = self.area.get_mut(row.saturating_mul(self.stride) + column) {
            *cell += value;
        }
    }

    /// The running sum, as coverage.
    ///
    /// The sum runs over the whole buffer rather than restarting each row,
    /// which is what the two padding columns are for: a closed contour's
    /// deltas cancel by the end of every row, so the sum arrives at the next
    /// row at zero on its own.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is clamped into 0..=255 on the line above the cast"
    )]
    pub(crate) fn finish(self) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.width.saturating_mul(self.height));
        let mut sum = 0.0f32;
        for row in self.area.chunks_exact(self.stride) {
            for (column, value) in row.iter().enumerate() {
                sum += value;
                if column < self.width {
                    let coverage = (abs(sum) * 255.0 + 0.5).clamp(0.0, 255.0);
                    pixels.push(coverage as u8);
                }
            }
        }
        pixels
    }
}
