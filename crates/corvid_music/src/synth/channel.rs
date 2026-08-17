//! What one of the sixteen channels is set to.
//!
//! A channel is where every message that is not a note lands: which preset it
//! plays, how loud, how far to one side, and -- with no bank loaded -- which
//! waveform stands in for an instrument. Kept apart from the mixer because
//! nothing here has anything to do with voices or with samples; it is a table of
//! settings and the arithmetic that turns a controller into a gain.

use crate::synth::midi::{BANK_SELECT_COARSE, BANK_SELECT_FINE, EXPRESSION, PAN, VOLUME};
use crate::synth::voice::Waveform;

/// The channel General MIDI reserves for percussion.
pub(crate) const DRUM_CHANNEL: u8 = 9;
/// The bank percussion presets live in.
pub(crate) const DRUM_BANK: u16 = 128;
/// How many banks a coarse bank-select step is worth.
const BANK_STRIDE: u16 = 128;

/// One channel's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Channel {
    /// Which bank a program change selects from.
    pub(crate) bank: u16,
    /// Which program it plays.
    pub(crate) program: u8,
    /// Controller 7.
    volume: u8,
    /// Controller 11.
    expression: u8,
    /// Controller 10.
    pan: u8,
    /// What it plays when there is no bank.
    pub(crate) waveform: Waveform,
}

impl Default for Channel {
    /// The state a General MIDI device powers up in: bank zero, program zero,
    /// volume 100, expression and pan centred.
    ///
    /// Not derived. A derived `volume` is zero, and a synthesizer that was
    /// silent until somebody remembered to send controller 7 is a bug that
    /// reaches a player as "the music never started".
    fn default() -> Self {
        Self {
            bank: 0,
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            waveform: Waveform::Sawtooth,
        }
    }
}

impl Channel {
    /// The gain this channel plays at.
    ///
    /// Volume squared and expression linear, which is the shape a player feels
    /// as even: halfway up the fader is a quarter of the power, and that is what
    /// sounds like half as loud.
    pub(crate) fn gain(self) -> f32 {
        let volume = f32::from(self.volume) / 127.0;
        let expression = f32::from(self.expression) / 127.0;
        volume * volume * expression
    }

    /// Where this channel sits, `-1.0` left to `1.0` right.
    pub(crate) fn pan(self) -> f32 {
        (f32::from(self.pan) - 64.0) / 63.5
    }

    /// Applies a controller.
    ///
    /// A controller this crate does not implement is accepted and ignored, which
    /// is what the specification asks of a device and is also the only sane
    /// answer: a bank's own setup messages are full of controllers nobody but
    /// its author has ever read.
    pub(crate) const fn control(&mut self, control: u8, value: u8) {
        match control {
            VOLUME => self.volume = value,
            EXPRESSION => self.expression = value,
            PAN => self.pan = value,
            BANK_SELECT_COARSE => {
                self.bank = (value as u16) * BANK_STRIDE + self.bank % BANK_STRIDE;
            }
            BANK_SELECT_FINE => self.bank = self.bank / BANK_STRIDE * BANK_STRIDE + value as u16,
            _ => {}
        }
    }

    /// Which bank a note on `channel` should be looked up in.
    ///
    /// Channel nine is percussion whatever its bank-select says, which is the
    /// one place General MIDI overrides a channel's own setting and the one
    /// place a synthesizer that ignored it would play a bassoon where a drum
    /// was meant.
    pub(crate) const fn bank_for(self, channel: u8) -> u16 {
        if channel == DRUM_CHANNEL {
            DRUM_BANK
        } else {
            self.bank
        }
    }
}

/// The gain a velocity plays at.
///
/// Squared, for the reason [`Channel::gain`] is.
pub(crate) fn velocity_gain(velocity: u8) -> f32 {
    let unit = f32::from(velocity.min(127)) / 127.0;
    unit * unit
}
