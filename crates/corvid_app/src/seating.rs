//! Which seat a client watches, and whether it plays it.

use corvid_behavior::PlayerId;

/// Where this client sits.
///
/// Not public, and that is a decision rather than an oversight: no signature a
/// caller can write mentions one. [`seat`](crate::App::seat) says which seat
/// and that this client plays it, [`spectating`](crate::App::spectating) says
/// it plays none, and between them they are the whole of what anybody outside
/// this crate can say about seating -- so a public enum here would be a type
/// every reader meets and nobody can use.
///
/// A client always watches a seat: the camera, the renderer and the ears belong
/// to somebody, and a run with nobody to look through has nothing to draw.
/// Whether it also submits an action for a seat is the other half, and it is
/// the half a spectator answers no to.
///
/// # Why the two are one type
///
/// Because they are not independent. A client that submitted for a seat it was
/// not looking through would be aiming with one player's camera and moving
/// another's body, and a client that watched nobody would have no camera at
/// all. Naming the pair as one value is what makes the second case
/// unrepresentable and the first deliberate: the seat is written once, and
/// [`playing`](Self::playing) is the only thing the two arms differ about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Seating {
    /// Submits for this seat, and watches it.
    Playing(PlayerId),
    /// Submits for nobody, and watches this seat.
    Watching(PlayerId),
}

impl Seating {
    /// The seat this client's camera, renderer and ears belong to.
    ///
    /// Always one, which is why the hooks that are handed a seat --
    /// [`action`](corvid_control::Controller::action),
    /// [`update`](corvid_control::Controller::update) -- never see an
    /// [`Option`]. A run that could not answer this would be a run with nothing
    /// to draw and nowhere to draw it from.
    #[must_use]
    pub(crate) const fn watched(self) -> PlayerId {
        match self {
            Self::Playing(seat) | Self::Watching(seat) => seat,
        }
    }

    /// The seat this client writes an action for, if it writes one.
    ///
    /// [`None`] is a spectator, and it is the answer the runtime branches on:
    /// nothing is asked of the controller and nothing is recorded, so the
    /// column the watched seat has is filled by whatever else fills it -- a peer
    /// on another machine, a bot, or the idle action a row nobody wrote holds.
    #[must_use]
    pub(crate) const fn playing(self) -> Option<PlayerId> {
        match self {
            Self::Playing(seat) => Some(seat),
            Self::Watching(_) => None,
        }
    }
}

impl Default for Seating {
    /// Playing the first seat, which is what a game run by one person is.
    ///
    /// Not derived: a derive would want a `Default` variant marked on the enum,
    /// and the default is not one of the two arms being more basic than the
    /// other -- it is a whole answer, seat included.
    fn default() -> Self {
        Self::Playing(PlayerId(0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use super::Seating;
    use corvid_behavior::PlayerId;

    #[test]
    fn a_player_watches_the_seat_it_plays() {
        let seated = Seating::Playing(PlayerId(2));
        assert_eq!(seated.watched(), PlayerId(2));
        assert_eq!(seated.playing(), Some(PlayerId(2)));
    }

    #[test]
    fn a_spectator_watches_a_seat_it_does_not_play() {
        let seated = Seating::Watching(PlayerId(1));
        assert_eq!(seated.watched(), PlayerId(1));
        assert_eq!(seated.playing(), None);
    }

    #[test]
    fn the_default_plays_the_first_seat() {
        assert_eq!(Seating::default(), Seating::Playing(PlayerId(0)));
    }
}
