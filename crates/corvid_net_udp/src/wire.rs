//! The packet format: what goes on the wire, and what comes back off it.
//!
//! Separated from the socket because it is the half with no I/O in it. Every
//! function here reads or writes bytes and nothing else, which is what lets
//! the parser be held to a stranger's packet without opening a port.

use corvid_net::{Channel, PeerId};

use crate::reliable::Piece;

/// What every packet this backend sends starts with, so that a stray packet on
/// the port is dropped rather than parsed.
pub(crate) const MAGIC: [u8; 4] = *b"CVDN";

/// Which version of the framing below. A packet naming another one is dropped:
/// two builds that disagree about the wire should fail to talk rather than
/// half-talk.
pub(crate) const VERSION: u8 = 1;
/// The kinds of packet.
pub(crate) mod kind {
    /// Hello, I am here. Carries nothing.
    pub(crate) const HELLO: u8 = 0;
    /// Hello back, which is what makes a peer reachable.
    pub(crate) const WELCOME: u8 = 1;
    /// Unreliable payload.
    pub(crate) const DATAGRAM: u8 = 2;
    /// One piece of a reliable channel's traffic.
    pub(crate) const PIECE: u8 = 3;
    /// What a receiver wants next on a channel.
    pub(crate) const ACK: u8 = 4;
    /// Going away.
    pub(crate) const BYE: u8 = 5;
}

/// The header every packet carries: magic, version, kind, and who sent it.
pub(crate) const HEADER: usize = 4 + 1 + 1 + 2;

/// The code for a [`Channel`] this build cannot name.
///
/// [`Channel`] is `#[non_exhaustive]`, so this crate has to answer for a
/// variant added to [`corvid_net`] after it was written. Mapping one onto
/// another channel's code would deliver its frames somewhere they do not
/// belong, which is worse than not carrying them: this code is one
/// [`channel_of`] refuses, so the far end drops the packet the way it drops
/// any other it cannot read. The unit test below is what makes that a
/// theoretical case rather than a live one: it round-trips every member of
/// [`Channel::ALL`] and fails the day one is added without a code here.
pub(crate) const UNKNOWN_CHANNEL: u8 = u8::MAX;

/// A [`Channel`] as one byte, and back.
///
/// The numbers are written out rather than taken from the variant's position,
/// because they are on the wire: two builds have to agree on them, and a
/// position would make reordering the variants a silent protocol change.
pub(crate) const fn channel_code(channel: Channel) -> u8 {
    match channel {
        Channel::Opening => 0,
        Channel::Transfer => 1,
        Channel::Control => 2,
        Channel::Chat => 3,
        _ => UNKNOWN_CHANNEL,
    }
}

/// The other direction, and [`None`] for a code from a build that has channels
/// this one does not.
pub(crate) const fn channel_of(code: u8) -> Option<Channel> {
    match code {
        0 => Some(Channel::Opening),
        1 => Some(Channel::Transfer),
        2 => Some(Channel::Control),
        3 => Some(Channel::Chat),
        _ => None,
    }
}

/// What one packet turned out to be, once its header was read.
pub(crate) enum Parsed<'a> {
    /// A greeting, either direction.
    Greeting { welcome: bool },
    /// Unreliable payload.
    Datagram(&'a [u8]),
    /// A piece of a reliable channel.
    Piece { code: u8, piece: Piece },
    /// An acknowledgement.
    Ack { code: u8, through: u32 },
    /// A goodbye.
    Bye,
}

/// Reads a packet's header and body, or answers [`None`] for anything this
/// build does not recognise.
///
/// Every field is bounds-checked and nothing is trusted. A packet on an open
/// port is the one input to this crate that arrives from a stranger.
pub(crate) fn parse(packet: &[u8]) -> Option<(PeerId, Parsed<'_>)> {
    if packet.len() < HEADER || packet.get(..4)? != MAGIC || *packet.get(4)? != VERSION {
        return None;
    }
    let kind = *packet.get(5)?;
    let from = PeerId(u16::from_le_bytes([*packet.get(6)?, *packet.get(7)?]));
    let body = packet.get(HEADER..)?;
    let what = match kind {
        kind::HELLO => Parsed::Greeting { welcome: false },
        kind::WELCOME => Parsed::Greeting { welcome: true },
        kind::DATAGRAM => Parsed::Datagram(body),
        kind::PIECE => {
            let code = *body.first()?;
            let more = *body.get(1)? != 0;
            let sequence =
                u32::from_le_bytes([*body.get(2)?, *body.get(3)?, *body.get(4)?, *body.get(5)?]);
            Parsed::Piece {
                code,
                piece: Piece {
                    sequence,
                    more,
                    bytes: body.get(6..)?.to_vec(),
                },
            }
        }
        kind::ACK => {
            let code = *body.first()?;
            let through =
                u32::from_le_bytes([*body.get(1)?, *body.get(2)?, *body.get(3)?, *body.get(4)?]);
            Parsed::Ack { code, through }
        }
        kind::BYE => Parsed::Bye,
        _ => return None,
    };
    Some((from, what))
}

/// The body of a piece packet: the channel, the flag, the sequence, the bytes.
pub(crate) fn piece_body(code: u8, piece: &Piece) -> Vec<u8> {
    let mut body = Vec::with_capacity(6 + piece.bytes.len());
    body.push(code);
    body.push(u8::from(piece.more));
    body.extend_from_slice(&piece.sequence.to_le_bytes());
    body.extend_from_slice(&piece.bytes);
    body
}

#[cfg(test)]
mod tests {
    use super::{UNKNOWN_CHANNEL, channel_code, channel_of};
    use corvid_net::Channel;

    #[test]
    fn every_channel_has_a_code_and_comes_back_as_itself() {
        // `Channel` is `#[non_exhaustive]`, so `channel_code` carries a `_` arm
        // and a channel added upstream would compile here while silently
        // becoming uncarryable. `Channel::ALL` grows with the variant list,
        // which is what turns that into this failing.
        for &channel in Channel::ALL {
            let code = channel_code(channel);
            assert_ne!(
                code, UNKNOWN_CHANNEL,
                "{channel} has no wire code in this backend"
            );
            assert_eq!(
                channel_of(code),
                Some(channel),
                "{channel} did not come back from its own code"
            );
        }
    }

    #[test]
    fn a_code_this_build_does_not_know_is_refused_rather_than_guessed() {
        assert_eq!(channel_of(UNKNOWN_CHANNEL), None);
        assert_eq!(channel_of(4), None);
    }
}
