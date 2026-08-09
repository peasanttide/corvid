#![doc = include_str!("../README.md")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the modules here are private, so pub(crate) and pub are equivalent -- pub(crate) is the one that says what is meant, and it is what rustc's unreachable_pub asks for"
)]

mod engine;
mod net;
mod queue;
mod records;
mod schedule;
mod tally;

pub use self::{
    net::{Endpoint, INBOX, MockNet, QUEUED},
    schedule::Schedule,
    tally::Tally,
};
