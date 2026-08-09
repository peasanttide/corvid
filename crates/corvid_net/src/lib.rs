#![doc = include_str!("../README.md")]

mod offline;
mod peer;
mod transport;

pub use self::{
    offline::Offline,
    peer::{Link, PeerId, PeerSet},
    transport::{Channel, DATAGRAM_LIMIT, Delivery, Lost, SendError, Transport},
};
