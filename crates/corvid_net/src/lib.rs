#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent — pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

mod mock;
mod transport;
pub mod udp;

pub use self::{
    mock::{Endpoint, INBOX, MockNet, Schedule, Tally},
    transport::{Channel, DATAGRAM_LIMIT, Delivery, Lost, PeerId, PeerSet, SendError, Transport},
};
