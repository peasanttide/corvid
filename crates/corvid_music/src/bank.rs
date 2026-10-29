//! A soundfont, decoded into the clips the synthesizer plays.

use std::sync::Arc;

use rameau_clip::Clip;
use rameau_software::Software;
use rameau_soundfont::SoundFont;

use crate::Error;

/// The instruments: a soundfont bank (`.sf2`, or `.sf3` with Ogg/Vorbis
/// samples) with every sample decoded to 16-bit PCM.
///
/// Decoding is the expensive part of starting music -- the embedded Unison bank
/// is 655 compressed samples, about a second of work in a release build -- so
/// a bank is decoded once and shared. Cloning one clones a handle.
#[derive(Clone, Debug)]
pub struct Bank {
    /// The presets, instruments and decoded samples.
    sound: Arc<SoundFont<Arc<Clip<i16>>>>,
}

impl Bank {
    /// Decodes the soundfont in `bytes`.
    ///
    /// # Errors
    ///
    /// [`Error::SoundFont`] when the bytes are not a well-formed soundfont or
    /// one of its compressed samples will not decode.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        // The software mixer has no Vorbis decoder, so the loader decodes each
        // compressed sample to PCM itself and hands it over, which is the
        // shape the mixer plays. The rate is the mixer's output rate and is
        // never read while loading: a clip keeps its own.
        let mut factory = Software::new(48_000);
        let sound = SoundFont::from_bytes_with(bytes, &mut factory)
            .map_err(|why| Error::SoundFont(why.to_string()))?;
        Ok(Self {
            sound: Arc::new(sound),
        })
    }

    /// Decodes the Unison general-MIDI bank this build carries.
    ///
    /// The bank is CC0-1.0, unlike the code that plays it, and adds about
    /// 6.6 MiB to a binary, which is why the `unison` feature is off by
    /// default.
    ///
    /// # Errors
    ///
    /// [`Error::SoundFont`] if the embedded bank will not decode, which is a
    /// corrupt build rather than anything a caller did.
    #[cfg(feature = "unison")]
    pub fn unison() -> Result<Self, Error> {
        Self::parse(rameau_unison::SOUNDFONT)
    }

    /// How many presets -- playable instruments -- the bank holds.
    #[must_use]
    pub fn presets(&self) -> usize {
        self.sound.presets.len()
    }

    /// The decoded bank, for a synthesizer to own a copy of.
    ///
    /// The copy is of the tables; the samples are shared.
    pub(crate) fn sound(&self) -> SoundFont<Arc<Clip<i16>>> {
        (*self.sound).clone()
    }
}
