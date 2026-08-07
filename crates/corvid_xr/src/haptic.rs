//! Rumble, and the effects worth naming.

use core::time::Duration;

use corvid_fixed::Factor16;
use serde::{Deserialize, Serialize};

/// One haptic pulse.
///
/// A runtime may round any of the three — a controller with one motor has one
/// frequency and takes the amplitude — so this describes what is wanted rather
/// than what is felt.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Haptic {
    /// How long it lasts.
    pub duration: Duration,
    /// How fast it buzzes, in hertz. Zero lets the runtime choose.
    pub frequency: u16,
    /// How hard.
    pub amplitude: Factor16,
}

impl Haptic {
    /// Nothing at all. What a game fires when an effect is switched off, so the
    /// call site keeps its shape.
    pub const SILENT: Self = Self {
        duration: Duration::ZERO,
        frequency: 0,
        amplitude: Factor16::MIN,
    };

    /// A button. Short, sharp, and the one most interactions want.
    pub const CLICK: Self = Self::new(Duration::from_millis(10), 200, Factor16::MAX);

    /// A boundary crossed — a cell edge under a pointer, a snap taken.
    pub const TICK: Self = Self::new(Duration::from_millis(6), 320, Factor16::from_bits(0x6000));

    /// Something landing. Low and longer, for a tower placed or a hit taken.
    pub const THUD: Self = Self::new(Duration::from_millis(60), 60, Factor16::MAX);

    /// A refusal: the placement that could not happen.
    pub const DENIED: Self = Self::new(Duration::from_millis(120), 90, Factor16::from_bits(0x9000));

    /// An effect from its three parts.
    #[must_use]
    #[inline]
    pub const fn new(duration: Duration, frequency: u16, amplitude: Factor16) -> Self {
        Self {
            duration,
            frequency,
            amplitude,
        }
    }

    /// The same effect at a different strength. What a comfort setting scales.
    #[must_use]
    #[inline]
    pub const fn at(self, amplitude: Factor16) -> Self {
        Self { amplitude, ..self }
    }

    /// Whether this effect would be felt at all.
    #[must_use]
    #[inline]
    pub const fn is_audible(self) -> bool {
        self.amplitude.to_bits() > 0 && self.duration.as_nanos() > 0
    }
}
