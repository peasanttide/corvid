//! How a lobby that has started hands its transport to the loop.
//!
//! A lobby is driven by a game's controller, on the thread the loop runs on,
//! and the loop is what has to start playing over the lobby's socket. Neither
//! holds the other, so the handover is a slot per thread: the lobby fills it
//! when the session starts, and the loop empties it before its next tick. One
//! slot per *thread* rather than per process, so two runs on two threads --
//! two machines in one test -- each find their own.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::{PeerId, Transport};

/// A session a lobby has started, ready for the loop.
#[derive(Debug)]
pub(crate) struct Started {
    /// The socket the lobby was talking over, which the session plays over.
    pub(crate) transport: Box<dyn Transport>,
    /// The seat this machine plays.
    pub(crate) seat: PlayerId,
    /// Which machine plays which seat.
    pub(crate) seats: BTreeMap<PeerId, PlayerId>,
    /// How many seats the session has.
    pub(crate) width: u16,
    /// The terms the host sent, encoded; [`None`] on the host itself, which
    /// writes them from the session it is playing and sends them to
    /// [`guests`](Self::guests).
    pub(crate) terms: Option<Vec<u8>>,
    /// Who the host sends the terms to. Empty on a guest.
    pub(crate) guests: Vec<PeerId>,
}

std::thread_local! {
    static SLOT: RefCell<Option<Started>> = const { RefCell::new(None) };
}

/// Leaves a started session for this thread's loop.
pub(crate) fn leave(started: Started) {
    SLOT.with(|slot| *slot.borrow_mut() = Some(started));
}

/// Takes a started session, if a lobby on this thread left one.
pub(crate) fn take() -> Option<Started> {
    SLOT.with(|slot| slot.borrow_mut().take())
}
