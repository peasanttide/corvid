//! What each [`SoundId`] sounds like, until there is an asset layer.

use corvid_sound::SoundId;

use crate::Timbre;

/// The lowest note a derived timbre lands on, in hertz.
///
/// The A below middle C, which is low enough to have body and high enough to
/// carry on a laptop speaker.
const ROOT: f32 = 220.0;

/// How many semitones the derived timbres walk before they repeat.
///
/// Two octaves. Wider and the top of the range whistles; narrower and two
/// sounds a game fires together are more likely to be the same note.
const SEMITONES: u32 = 24;

/// Which recording each [`SoundId`] names -- except that there are no
/// recordings, so it is which [`Timbre`].
///
/// A game registers the sounds it cares about and leaves the rest. What it
/// leaves is **not silence**: an identifier nobody described is played as a
/// knock at a pitch derived from its number, so a sound the game fired is a
/// sound the game hears. A missing entry that played nothing would be
/// indistinguishable from a cue that was never fired, and telling those apart
/// is most of what a person debugging audio is doing.
///
/// That derivation is a placeholder in the strong sense. It has no idea what
/// any sound means, and which note a sound ends up on is an accident of the
/// number the game gave it. Nothing here reads a catalogue from an asset file,
/// and a game that wants its sounds chosen deliberately supplies its own
/// mapping.
///
/// ```
/// use corvid_audio::{Catalogue, Timbre};
/// use corvid_sound::SoundId;
///
/// const THUD: SoundId = SoundId(2);
/// const CHIME: SoundId = SoundId(7);
///
/// let catalogue = Catalogue::new()
///     .with(THUD, Timbre::knock(90.0).with_decay(0.18).with_bite(0.7))
///     .with(CHIME, Timbre::knock(880.0).with_decay(1.5).with_bite(0.05));
///
/// assert_eq!(catalogue.timbre(THUD).hertz, 90.0);
///
/// // And a sound nobody described is audible rather than missing.
/// assert!(catalogue.timbre(SoundId(1234)).hertz > 0.0);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalogue {
    /// Every described sound, in the order it was described. Searched
    /// linearly, because this is read on the game's thread once per cue and a
    /// game has tens of sounds rather than thousands.
    entries: Vec<(SoundId, Timbre)>,
}

impl Catalogue {
    /// A catalogue that describes nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Describes `sound`.
    ///
    /// Describing the same identifier twice keeps the first description, which
    /// is what makes a table built by appending read the way it is written.
    #[must_use]
    pub fn with(mut self, sound: SoundId, timbre: Timbre) -> Self {
        self.entries.push((sound, timbre));
        self
    }

    /// What `sound` sounds like, described or derived.
    #[must_use]
    pub fn timbre(&self, sound: SoundId) -> Timbre {
        self.entries
            .iter()
            .find(|(id, _)| *id == sound)
            .map_or_else(|| derived(sound), |(_, timbre)| *timbre)
    }

    /// How many sounds have been described.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been described.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The knock an undescribed identifier is played as.
///
/// A chromatic step per identifier over two octaves, so consecutive numbers are
/// audibly different and a game that numbered its sounds from one gets a scale
/// rather than a drone.
fn derived(sound: SoundId) -> Timbre {
    let step = sound.0 % SEMITONES;
    // A count of semitones is at most twenty-three.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the modulus above puts this below twenty-four, where every integer has an exact f32"
    )]
    let step = step as f32;
    Timbre::knock(ROOT * (step / 12.0).exp2())
}

#[cfg(test)]
mod tests {
    //! What a catalogue answers for what it was not told.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "every frequency compared here is a literal that was stored and read back, or one derived from an integer by a multiply -- the exact bits are what is being asserted, and a tolerance would pass on a table that had lost an entry"
    )]

    use super::{Catalogue, ROOT, SEMITONES, derived};
    use crate::Timbre;
    use corvid_sound::SoundId;

    #[test]
    fn a_described_sound_is_the_description_and_not_the_derivation() {
        let described = Timbre::knock(123.0);
        let catalogue = Catalogue::new().with(SoundId(5), described);
        assert_eq!(catalogue.timbre(SoundId(5)), described);
        assert_ne!(catalogue.timbre(SoundId(5)), derived(SoundId(5)));
    }

    #[test]
    fn describing_a_sound_twice_keeps_the_first_description() {
        let catalogue = Catalogue::new()
            .with(SoundId(5), Timbre::knock(100.0))
            .with(SoundId(5), Timbre::knock(200.0));
        assert_eq!(catalogue.timbre(SoundId(5)).hertz, 100.0);
        assert_eq!(catalogue.len(), 2);
    }

    #[test]
    fn an_undescribed_sound_is_audible_and_two_of_them_differ() {
        // Both halves matter. Silence would make a fired cue and a missing
        // entry look the same, and one note for every identifier would make a
        // game with six sounds a game with one.
        let catalogue = Catalogue::new();
        let first = catalogue.timbre(SoundId(1));
        let second = catalogue.timbre(SoundId(2));
        assert!(first.hertz >= ROOT);
        assert_ne!(first.hertz, second.hertz);
    }

    #[test]
    fn the_derivation_stays_inside_two_octaves_however_large_the_number() {
        // A `SoundId` is a `u32`, so without the modulus a large one would be
        // an inaudible whistle or an infinity.
        for sound in [
            SoundId(0),
            SoundId(SEMITONES - 1),
            SoundId(SEMITONES),
            SoundId(u32::MAX),
        ] {
            let hertz = derived(sound).hertz;
            assert!(
                (ROOT..ROOT * 4.0).contains(&hertz),
                "{sound} derived {hertz} Hz",
            );
        }
        // And it repeats at the octave pair rather than drifting.
        assert_eq!(derived(SoundId(0)), derived(SoundId(SEMITONES)));
    }
}
