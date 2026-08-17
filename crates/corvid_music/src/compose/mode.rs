//! The ladder of modes, and the resolution of a scale degree into a MIDI key.

use crate::num;

/// One of the six diatonic modes, ordered bright to dark.
///
/// The order is the whole point of the type. Reading down [`Mode::LADDER`],
/// exactly one scale degree flattens at each rung -- the fourth, then the
/// seventh, then the third, then the sixth, then the second -- so a parameter
/// that walks the ladder darkens the music continuously rather than in jumps,
/// and the third in particular is major for the first three rungs and minor for
/// the last three. That is what [`Mode::third`] reports and what makes "darker"
/// a claim a test can check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub enum Mode {
    /// Sharp fourth: the brightest rung.
    Lydian,
    /// The major scale.
    #[default]
    Ionian,
    /// Flat seventh.
    Mixolydian,
    /// Flat seventh and flat third.
    Dorian,
    /// The natural minor scale.
    Aeolian,
    /// Flat second as well: the darkest rung.
    Phrygian,
}

impl Mode {
    /// Every mode, brightest first.
    ///
    /// ```
    /// use corvid_music::Mode;
    ///
    /// let thirds: Vec<i8> = Mode::LADDER.iter().map(|mode| mode.third()).collect();
    /// assert_eq!(thirds, [4, 4, 4, 3, 3, 3]);
    /// ```
    pub const LADDER: [Self; 6] = [
        Self::Lydian,
        Self::Ionian,
        Self::Mixolydian,
        Self::Dorian,
        Self::Aeolian,
        Self::Phrygian,
    ];

    /// The semitone above the tonic of each of the seven degrees.
    #[must_use]
    pub const fn semitones(self) -> [i8; 7] {
        match self {
            Self::Lydian => [0, 2, 4, 6, 7, 9, 11],
            Self::Ionian => [0, 2, 4, 5, 7, 9, 11],
            Self::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
            Self::Dorian => [0, 2, 3, 5, 7, 9, 10],
            Self::Aeolian => [0, 2, 3, 5, 7, 8, 10],
            Self::Phrygian => [0, 1, 3, 5, 7, 8, 10],
        }
    }

    /// The semitone of the third degree: `4` for a major third, `3` for a minor
    /// one.
    ///
    /// Named because it is the interval a listener hears the mode *as*, and
    /// because "darkness flattens the third" is a claim about this number.
    #[must_use]
    pub const fn third(self) -> i8 {
        self.semitones()[2]
    }

    /// Where this mode sits on the ladder, `0` brightest and `5` darkest.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Lydian => 0,
            Self::Ionian => 1,
            Self::Mixolydian => 2,
            Self::Dorian => 3,
            Self::Aeolian => 4,
            Self::Phrygian => 5,
        }
    }

    /// The rung `darkness` names, with `0.0` the brightest and `1.0` the
    /// darkest.
    ///
    /// The six rungs divide the interval evenly, so a parameter sweep spends the
    /// same time in each. Values outside `0.0 ..= 1.0` and a `NaN` all land on a
    /// rung rather than escaping, because this is called with a number a game
    /// computed and no arithmetic upstream is checked here.
    ///
    /// ```
    /// use corvid_music::Mode;
    ///
    /// assert_eq!(Mode::from_darkness(0.0), Mode::Lydian);
    /// assert_eq!(Mode::from_darkness(1.0), Mode::Phrygian);
    /// assert!(Mode::from_darkness(0.9).third() < Mode::from_darkness(0.1).third());
    /// ```
    #[must_use]
    pub fn from_darkness(darkness: f32) -> Self {
        let rung = num::count(libm::floorf(darkness.clamp(0.0, 1.0) * 6.0));
        *Self::LADDER.get(rung.min(5)).unwrap_or(&Self::Ionian)
    }

    /// Whether `semitone`, taken from the tonic, is in this scale.
    #[must_use]
    pub fn contains_semitone(self, semitone: i8) -> bool {
        let folded = semitone.rem_euclid(12);
        self.semitones().contains(&folded)
    }
}

/// A note written as a place in a scale rather than as a pitch.
///
/// This is why a motif survives transposition, a mode change and an inversion
/// without being re-edited: a `Step` says "the third degree, a semitone flat, an
/// octave up", and what that sounds like is decided when it is resolved. An
/// [`alteration`](Self::alteration) stays an alteration through every
/// transformation, so a chromatic note that gave a tune its character is still
/// chromatic after the tune has been turned upside down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Step {
    /// The scale degree, `0` for the tonic. Values outside `0 ..= 6` wrap and
    /// carry an octave with them.
    pub degree: i8,
    /// Semitones away from what the mode gives that degree.
    pub alteration: i8,
    /// Octaves above the resolving octave.
    pub octave: i8,
}

impl Step {
    /// The unaltered `degree` in the resolving octave.
    #[must_use]
    pub const fn new(degree: i8) -> Self {
        Self {
            degree,
            alteration: 0,
            octave: 0,
        }
    }

    /// Bends it by `alteration` semitones.
    #[must_use]
    pub const fn altered(self, alteration: i8) -> Self {
        Self { alteration, ..self }
    }

    /// Moves it by `octave` octaves.
    #[must_use]
    pub const fn octave(self, octave: i8) -> Self {
        Self { octave, ..self }
    }

    /// Resolves it into a MIDI key.
    ///
    /// `tonic` is a pitch class, `0` for C, and `octave` is the scientific
    /// octave the tonic sits in, so the tonic of C in octave 4 is middle C.
    /// The answer is clamped into `0 ..= 127`: a line transposed off the end of
    /// the keyboard flattens out rather than wrapping round to the other end.
    ///
    /// ```
    /// use corvid_music::{Mode, Step};
    ///
    /// assert_eq!(Step::new(0).key(0, Mode::Ionian, 4), 60);
    /// assert_eq!(Step::new(7).key(0, Mode::Ionian, 4), 72);
    /// assert_eq!(Step::new(2).key(0, Mode::Aeolian, 4), 63);
    /// ```
    #[must_use]
    pub fn key(self, tonic: u8, mode: Mode, octave: i8) -> u8 {
        let folded = self.degree.rem_euclid(7);
        let wrap = i32::from(self.degree.div_euclid(7));
        let semitone = i32::from(
            mode.semitones()
                .get(usize::from(folded.unsigned_abs()))
                .copied()
                .unwrap_or(0),
        );
        let octaves = i32::from(octave) + 1 + i32::from(self.octave) + wrap;
        let key = octaves * 12 + i32::from(tonic % 12) + semitone + i32::from(self.alteration);
        u8::try_from(key.clamp(0, i32::from(num::MAX_KEY))).unwrap_or(0)
    }
}
