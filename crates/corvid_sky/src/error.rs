//! The three ways an input to this crate can fail to name a place or a moment.

/// What went wrong turning a caller's numbers into a moment or a site.
///
/// Nothing in the ephemeris itself fails. A series evaluated at any argument
/// answers something, and whether that something is *accurate* is a question
/// about the epoch and not about the call; [`crate::Instant`] documents the
/// range each series was fitted over. So every variant here is a value that
/// did not describe a moment or a place at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SkyError {
    /// A calendar field outside the range the proleptic Gregorian calendar
    /// gives it: a month outside `1 ..= 12`, a day outside `1 ..= 31`, an hour
    /// outside `0 ..= 23`, or a minute or second outside `0 ..= 59`.
    ///
    /// The named field is the first one found wrong, so correcting it can
    /// reveal a second.
    #[error("the calendar field `{field}` is outside the range a civil date gives it")]
    Calendar {
        /// Which field: `"month"`, `"day"`, `"hour"`, `"minute"` or
        /// `"second"`.
        field: &'static str,
    },

    /// A latitude outside `-90 ..= 90` degrees, a longitude outside
    /// `-360 ..= 360`, or an elevation that is not finite.
    #[error("a site needs a latitude within +/-90 degrees and a finite elevation")]
    Site,

    /// A number that was not finite where the arithmetic needs one: a `NaN`
    /// Julian day, an infinite altitude.
    #[error("a value that is not finite cannot name a moment or a direction")]
    NotFinite,
}
