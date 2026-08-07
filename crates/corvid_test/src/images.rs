//! Comparing a captured frame against a frozen one, with a tolerance.
//!
//! # Why this is not `matches_goldens`
//!
//! Everything else a run writes down is compared byte for byte, because
//! everything else a run writes down is integer data a simulation computed. A
//! frame is not: it is what a *driver* rasterised, and two drivers disagree
//! about the last bit of a shaded pixel for reasons that have nothing to do
//! with the game. Comparing PNG bytes would report a failure on every machine
//! that is not the one that blessed them.
//!
//! So a frame is a **perceptual** golden. [`images_agree`] takes a
//! [`Tolerance`], and the honest thing to say about what a passing comparison
//! proves is on that type. The bit-exact golden that remains is the hash trace,
//! which is compared through [`matches_goldens`](crate::matches_goldens) on
//! every target — a regression in what the simulation computes fails
//! everywhere, and a regression in how a driver shades it fails in one place,
//! deliberately.

use std::{fmt, fs, io, path::Path};

use crate::goldens::BLESS;

/// How far apart two frames may be and still be the same frame.
///
/// Two numbers, because the two failures a screenshot golden has are different
/// shapes. A driver that rounds a shaded pixel differently is off by a little
/// everywhere, which [`channel`](Self::channel) absorbs. A frame drawn from a
/// state one tick out is off by a lot somewhere, which
/// [`differing`](Self::differing) catches however small the shading difference
/// is.
///
/// # What a comparison at a tolerance proves, and what it does not
///
/// It proves that the frame is *about* the same picture: no more of the frame
/// than the allowance changed by more than the allowance.
/// It does **not** prove that the frame is the one the game meant to draw, and
/// no screenshot comparison can — a cube drawn at the right place in the wrong
/// colour, or the right colour at the wrong depth, passes anything loose enough
/// to survive two drivers.
///
/// What that is worth is a regression detector for geometry, framing and
/// gross shading. What it is not worth is a determinism claim, and this crate
/// deliberately keeps the two apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tolerance {
    /// How far one channel of one pixel may move before that pixel counts as
    /// having changed at all, in eight-bit levels.
    pub channel: u8,
    /// How many of the frame's pixels may move further than
    /// [`channel`](Self::channel), in parts per million.
    pub differing: u32,
}

impl Tolerance {
    /// No difference at all.
    ///
    /// This is the arm to pin to the software adapter. Two runs on one
    /// `lavapipe` produce one answer —
    /// `corvid_render/tests/offscreen.rs::the_same_frame_twice_is_the_same_bytes_and_survives_a_png`
    /// is what says so — so a build machine can demand equality and catch a
    /// change a tolerance would absorb. Asking for it on hardware nobody
    /// blessed the golden on is asking for a red test about a driver.
    pub const EXACT: Self = Self {
        channel: 0,
        differing: 0,
    };

    /// What two drivers rasterising the same frame are worth.
    ///
    /// Eight levels of 255 is about three per cent of a channel, which absorbs
    /// a different rounding in a shader and a different filter on an edge. Two
    /// per cent of the pixels is roughly the outline of a shape a few dozen
    /// pixels across, which absorbs an edge landing on the other side of a
    /// sample point and does not absorb the shape being somewhere else.
    ///
    /// **Both numbers are judgement rather than measurement, and it is worth
    /// saying so.** The one measurement this workspace has is
    /// `examples/headless/tests/goldens.rs`, which on a machine with a GPU
    /// compares the frames its software rasteriser blessed against the same
    /// frames from that GPU and prints the spread. On the machine this was
    /// written on — Mesa `lavapipe` against an NVIDIA RTX 4090, both through
    /// Vulkan — that spread is **zero**: the two drivers produce identical
    /// bytes for a flat-shaded cube. So these numbers are not derived from a
    /// disagreement anybody has seen; they are room left for one, on the
    /// grounds that two drivers agreeing on this scene is not two drivers
    /// agreeing on every scene. Re-run that test on a third driver and the
    /// line it prints is the evidence to set them from.
    pub const PERCEPTUAL: Self = Self {
        channel: 8,
        differing: 20_000,
    };
}

impl fmt::Display for Tolerance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "up to {} of 255 per channel over up to {} pixels per million",
            self.channel, self.differing,
        )
    }
}

/// One decoded frame: eight-bit RGBA, row by row from the top.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Frozen {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// Four bytes per pixel.
    pub pixels: Vec<u8>,
}

/// Reads a PNG written by `corvid_render::Image::to_png`.
///
/// # Errors
///
/// [`Different::Unreadable`] if the file will not open, and
/// [`Different::Malformed`] if it is not an eight-bit RGBA PNG — which is
/// everything a Corvid capture writes, so a file that is not one came from
/// somewhere else.
pub fn read_png(path: &Path) -> Result<Frozen, Different> {
    let file = fs::File::open(path).map_err(|why| Different::Unreadable {
        path: path.to_path_buf(),
        why,
    })?;
    let malformed = |why: String| Different::Malformed {
        path: path.to_path_buf(),
        why,
    };
    let mut reader = png::Decoder::new(io::BufReader::new(file))
        .read_info()
        .map_err(|why| malformed(why.to_string()))?;
    let mut pixels = vec![
        0;
        reader.output_buffer_size().ok_or_else(|| malformed(
            "the frame does not fit in memory".to_owned()
        ))?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|why| malformed(why.to_string()))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(malformed(format!(
            "it is {:?} at {:?} rather than eight-bit RGBA",
            info.color_type, info.bit_depth,
        )));
    }
    pixels.truncate(info.buffer_size());
    Ok(Frozen {
        width: info.width,
        height: info.height,
        pixels,
    })
}

/// Compares a captured frame against a frozen one, and rewrites the frozen one
/// under [`BLESS`].
///
/// The two paths are in the order they are for the reason
/// [`matches_goldens`](crate::matches_goldens)'s are: the capture is what
/// happened and the golden is what was expected, and blessing copies the first
/// over the second.
///
/// # Blessing
///
/// With [`BLESS`] set to anything non-empty, a golden that no longer agrees is
/// rewritten from the capture and **the call still fails**, saying so. That is
/// deliberate twice over: a blessing run that went green would tell nobody what
/// it changed, and a CI job with the variable set by accident would go green
/// forever. Run it again to see it pass.
///
/// # Errors
///
/// [`Different::Unreadable`] or [`Different::Malformed`] for a file that will
/// not read, [`Different::Size`] for two frames of different shapes — which no
/// tolerance covers, because there is no pixel-to-pixel correspondence to
/// measure — [`Different::Pixels`] for two frames that are further apart than
/// `tolerance` allows, and [`Different::Rewritten`] when a blessing run
/// replaced the golden.
pub fn images_agree(capture: &Path, golden: &Path, tolerance: Tolerance) -> Result<(), Different> {
    let taken = read_png(capture)?;
    let blessing = std::env::var_os(BLESS).is_some_and(|value| !value.is_empty());

    let bless = |why: Box<Different>| -> Different {
        if !blessing {
            return *why;
        }
        // The directory first, because the case this path exists for — the
        // first blessing of a frame nobody has frozen yet — is also the case
        // where `goldens/frames/` does not exist. `fs::copy` does not make one,
        // so without this the first blessing answers the same "no such file"
        // it was called to fix. `matches_goldens` has done this since it was
        // written; this is the same rule for the other kind of golden.
        if let Some(parent) = golden.parent()
            && let Err(oops) = fs::create_dir_all(parent)
        {
            return Different::Unreadable {
                path: parent.to_path_buf(),
                why: oops,
            };
        }
        match fs::copy(capture, golden) {
            Ok(_) => Different::Rewritten {
                golden: golden.to_path_buf(),
                why,
            },
            Err(oops) => Different::Unreadable {
                path: golden.to_path_buf(),
                why: oops,
            },
        }
    };

    // A missing golden is a finding rather than an error, and under a blessing
    // run it is the first blessing of a new frame.
    let frozen = match read_png(golden) {
        Ok(frozen) => frozen,
        Err(Different::Unreadable { path, why }) if why.kind() == io::ErrorKind::NotFound => {
            return Err(bless(Box::new(Different::Unreadable { path, why })));
        }
        Err(why) => return Err(why),
    };

    if (taken.width, taken.height) != (frozen.width, frozen.height) {
        return Err(bless(Box::new(Different::Size {
            golden: golden.to_path_buf(),
            captured: (taken.width, taken.height),
            recorded: (frozen.width, frozen.height),
        })));
    }

    let mut worst = 0u8;
    let mut where_worst = 0usize;
    let mut differing = 0u64;
    for (index, (one, two)) in taken
        .pixels
        .chunks_exact(4)
        .zip(frozen.pixels.chunks_exact(4))
        .enumerate()
    {
        let apart = one
            .iter()
            .zip(two)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        if apart > worst {
            worst = apart;
            where_worst = index;
        }
        // Counted only past `channel`, which is what makes the two numbers the
        // two different failures they are documented as. Counting every pixel
        // that differed *at all* and then also demanding `worst <= channel`
        // made `channel` absorb nothing: a driver that rounds the whole frame
        // one level differently is inside `channel` everywhere and yet trips a
        // budget quoted in parts per million, so the perceptual arm rejected
        // exactly the difference it exists to allow.
        if apart > tolerance.channel {
            differing += 1;
        }
    }

    let total = u64::from(taken.width) * u64::from(taken.height);
    // Rounded down deliberately: on a frame of fewer than a million pixels a
    // tolerance in parts per million that rounded up would allow one whole
    // pixel more than it says, and "no pixels at all" has to stay expressible.
    let allowed = total.saturating_mul(u64::from(tolerance.differing)) / 1_000_000;
    if differing <= allowed {
        return Ok(());
    }

    let width = taken.width.max(1);
    Err(bless(Box::new(Different::Pixels {
        golden: golden.to_path_buf(),
        worst,
        at: (
            u32::try_from(where_worst).unwrap_or(u32::MAX) % width,
            u32::try_from(where_worst).unwrap_or(u32::MAX) / width,
        ),
        differing,
        total,
        tolerance,
    })))
}

/// A captured frame and a frozen one are not the same picture.
#[derive(Debug)]
#[non_exhaustive]
pub enum Different {
    /// A file would not open.
    Unreadable {
        /// Which one. `io::Error` does not carry the path it was about.
        path: std::path::PathBuf,
        /// Why not.
        why: io::Error,
    },
    /// A file opened and is not an eight-bit RGBA PNG.
    Malformed {
        /// Which one.
        path: std::path::PathBuf,
        /// What the decoder said.
        why: String,
    },
    /// The two frames are different shapes, which no tolerance covers.
    Size {
        /// Where the frozen one lives.
        golden: std::path::PathBuf,
        /// How big the capture is.
        captured: (u32, u32),
        /// How big the golden is.
        recorded: (u32, u32),
    },
    /// The two frames are further apart than the tolerance allows.
    Pixels {
        /// Where the frozen one lives.
        golden: std::path::PathBuf,
        /// The largest single-channel difference found.
        worst: u8,
        /// Where it was, as a column and a row from the top left.
        at: (u32, u32),
        /// How many pixels differ at all.
        differing: u64,
        /// How many there are.
        total: u64,
        /// What was allowed.
        tolerance: Tolerance,
    },
    /// [`BLESS`] was set, and the golden was rewritten from the capture.
    Rewritten {
        /// Which one.
        golden: std::path::PathBuf,
        /// What it disagreed about before it was replaced.
        why: Box<Self>,
    },
}

impl fmt::Display for Different {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, why } => write!(f, "{}: {why}", path.display()),
            Self::Malformed { path, why } => write!(
                f,
                "{} is not a frame this workspace wrote: {why}",
                path.display(),
            ),
            Self::Size {
                golden,
                captured,
                recorded,
            } => write!(
                f,
                "{} is {}x{} and the capture is {}x{}, so there is no pixel to \
                 compare with which and no tolerance that could cover it",
                golden.display(),
                recorded.0,
                recorded.1,
                captured.0,
                captured.1,
            ),
            Self::Pixels {
                golden,
                worst,
                at,
                differing,
                total,
                tolerance,
            } => write!(
                f,
                "{} and the captured frame are further apart than {tolerance}: \
                 {differing} of {total} pixels moved further than that, worst \
                 by {worst} of 255 at column {} row {}. If that is deliberate, \
                 {BLESS}=1 records \
                 the capture over it — but read what a frame golden is worth \
                 first, because a frame moving is as often a driver as a game.",
                golden.display(),
                at.0,
                at.1,
            ),
            Self::Rewritten { golden, why } => write!(
                f,
                "{BLESS} was set: {} was rewritten from the capture, which \
                 before that disagreed as follows. {why}",
                golden.display(),
            ),
        }
    }
}

impl std::error::Error for Different {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { why, .. } => Some(why),
            Self::Rewritten { why, .. } => Some(why),
            Self::Malformed { .. } | Self::Size { .. } | Self::Pixels { .. } => None,
        }
    }
}
