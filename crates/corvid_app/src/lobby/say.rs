//! What machines in a lobby say to each other, over
//! [`Channel::Opening`](corvid_net::Channel).
//!
//! The host is the only one who decides anything: who is in, which seat each
//! sits in, and when the session starts. A guest says hello, says whether it is
//! ready, and otherwise listens.

use std::string::String;
use std::vec::Vec;

use serde::{Deserialize, Serialize};

/// One frame of lobby talk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Say {
    /// A guest asking in: its name, and the game it is running, so a host
    /// running something else can say no.
    Hello {
        /// What the guest calls itself.
        name: String,
        /// The game's name, as [`Game::NAME`](crate::Game::NAME) gives it.
        game: String,
    },
    /// The host, whenever anything changes: everyone in, and how many seats
    /// the session has.
    Room {
        /// Everyone in the lobby, the host first.
        members: Vec<Seen>,
        /// How many seats the session will have.
        seats: u16,
    },
    /// A guest saying whether it is ready to start.
    Ready {
        /// Whether it is.
        ready: bool,
    },
    /// The host turning a guest away, and why.
    Refused {
        /// A sentence a person can read.
        why: String,
    },
    /// Going: a guest telling the host, or the host telling every guest.
    Leave,
    /// The host starting the session: the terms of the opening every
    /// machine plays from, encoded.
    Start {
        /// The level's name and digest, the rules, the roster, the seed and
        /// the schema, as the loop encodes them.
        terms: Vec<u8>,
    },
}

/// One member, as the host describes it to everyone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Seen {
    /// Its [`PeerId`](corvid_net::PeerId) number.
    pub(super) peer: u16,
    /// What it calls itself.
    pub(super) name: String,
    /// The seat it will play.
    pub(super) seat: u16,
    /// Whether it has said it is ready. The host always is.
    pub(super) ready: bool,
    /// Where the host hears it from, so the other guests can reach it too.
    pub(super) address: Option<String>,
}
