//! Triads and sevenths built by stacking thirds, and the cadences they make.

use crate::compose::Mode;

/// What kind of triad a chord is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Quality {
    /// Major third, perfect fifth.
    #[default]
    Major,
    /// Minor third, perfect fifth.
    Minor,
    /// Minor third, diminished fifth.
    Diminished,
    /// Major third, augmented fifth.
    Augmented,
}

/// A chord, as a degree of the current mode rather than as a set of pitches.
///
/// Everything downstream resolves it through the mode it was built in, so the
/// same progression is a different set of chords in a different mode without
/// anything having to say so. A chord carries no inversion and no voicing:
/// which note is in the bass and which octave each line takes is decided when
/// the bar is written, because those are answers to where the previous bar left
/// the lines and not to what the harmony is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Chord {
    /// Scale degree of the root, `0 ..= 6`.
    pub degree: u8,
    /// Whether the triad is major, minor, diminished or augmented.
    pub quality: Quality,
    /// Whether the seventh is stacked on as well.
    pub seventh: bool,
}

impl Chord {
    /// The chord built by stacking thirds on `degree` of `mode`.
    ///
    /// ```
    /// use corvid_music::{Chord, Mode, Quality};
    ///
    /// assert_eq!(Chord::on(0, Mode::Ionian, false).quality, Quality::Major);
    /// assert_eq!(Chord::on(1, Mode::Ionian, false).quality, Quality::Minor);
    /// assert_eq!(Chord::on(6, Mode::Ionian, false).quality, Quality::Diminished);
    /// assert_eq!(Chord::on(0, Mode::Aeolian, false).quality, Quality::Minor);
    /// ```
    #[must_use]
    pub fn on(degree: u8, mode: Mode, seventh: bool) -> Self {
        let degree = degree % 7;
        let scale = mode.semitones();
        let at = |offset: u8| -> i8 {
            scale
                .get(usize::from((degree + offset) % 7))
                .copied()
                .unwrap_or(0)
        };
        let root = at(0);
        let third = (at(2) - root).rem_euclid(12);
        let fifth = (at(4) - root).rem_euclid(12);
        let quality = match (third, fifth) {
            (4, 7) => Quality::Major,
            (3, 7) => Quality::Minor,
            (4, 8) => Quality::Augmented,
            _ => Quality::Diminished,
        };
        Self {
            degree,
            quality,
            seventh,
        }
    }

    /// The scale degrees this chord is made of, root first.
    ///
    /// ```
    /// use corvid_music::{Chord, Mode};
    ///
    /// let dominant = Chord::on(4, Mode::Ionian, true);
    /// assert_eq!(dominant.tones().collect::<Vec<u8>>(), [4, 6, 1, 3]);
    /// ```
    pub fn tones(self) -> impl Iterator<Item = u8> {
        let count = if self.seventh { 4 } else { 3 };
        (0..count).map(move |index| (self.degree + index * 2) % 7)
    }

    /// Whether `degree` is one of this chord's tones, wrapping octaves.
    #[must_use]
    pub fn contains(self, degree: i8) -> bool {
        let folded = degree.rem_euclid(7);
        u8::try_from(folded).is_ok_and(|folded| self.tones().any(|tone| tone == folded))
    }

    /// The roman numeral, upper case for a major or augmented triad and lower
    /// case otherwise, with `o` for a diminished fifth and `7` for a seventh.
    ///
    /// For a debug overlay and for reading a test's failure, which is why it is
    /// plain ASCII rather than the degree sign the printed convention uses.
    ///
    /// ```
    /// use corvid_music::{Chord, Mode};
    ///
    /// assert_eq!(Chord::on(4, Mode::Ionian, true).roman(), "V7");
    /// assert_eq!(Chord::on(6, Mode::Ionian, false).roman(), "viio");
    /// ```
    #[must_use]
    pub fn roman(self) -> &'static str {
        const UPPER: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
        const LOWER: [&str; 7] = ["i", "ii", "iii", "iv", "v", "vi", "vii"];
        const DIMINISHED: [&str; 7] = ["io", "iio", "iiio", "ivo", "vo", "vio", "viio"];
        const UPPER_SEVENTH: [&str; 7] = ["I7", "II7", "III7", "IV7", "V7", "VI7", "VII7"];
        const LOWER_SEVENTH: [&str; 7] = ["i7", "ii7", "iii7", "iv7", "v7", "vi7", "vii7"];
        const DIMINISHED_SEVENTH: [&str; 7] =
            ["io7", "iio7", "iiio7", "ivo7", "vo7", "vio7", "viio7"];
        let table = match (self.quality, self.seventh) {
            (Quality::Major | Quality::Augmented, false) => UPPER,
            (Quality::Major | Quality::Augmented, true) => UPPER_SEVENTH,
            (Quality::Minor, false) => LOWER,
            (Quality::Minor, true) => LOWER_SEVENTH,
            (Quality::Diminished, false) => DIMINISHED,
            (Quality::Diminished, true) => DIMINISHED_SEVENTH,
        };
        table
            .get(usize::from(self.degree % 7))
            .copied()
            .unwrap_or("?")
    }
}

/// How a phrase closed.
///
/// A bar carries one only when the cadence actually landed there, which is what
/// makes [`Composer::interrupt`](crate::Composer::interrupt) and the deferral in
/// [`Composer::next_bar`](crate::Composer::next_bar) legible: while tension is
/// rising every bar's cadence is `None`, and the bar that finally resolves is
/// the one that names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Cadence {
    /// Dominant to tonic: the close a listener is waiting for.
    Authentic,
    /// Subdominant to tonic.
    Plagal,
    /// Dominant to something that is not the tonic, which is how a deferral
    /// sounds from inside.
    Deceptive,
    /// Stopping on the dominant, which closes a phrase without closing a
    /// paragraph.
    Half,
}

impl Cadence {
    /// Whether this cadence resolves, as against holding the tension open.
    #[must_use]
    pub const fn resolves(self) -> bool {
        matches!(self, Self::Authentic | Self::Plagal)
    }
}
