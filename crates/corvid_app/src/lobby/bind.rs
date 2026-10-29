//! Where a lobby's socket is bound, and who a guest is on it.

use std::io;
use std::net::{Ipv4Addr, ToSocketAddrs};

use corvid_net::PeerId;

/// The address to bind to reach `address` from: the loopback address for a
/// lobby on it, any address otherwise.
///
/// # Errors
///
/// Whatever resolving the address says.
pub(crate) fn here_for(address: &str) -> io::Result<Ipv4Addr> {
    let mut resolved = address.to_socket_addrs()?;
    let local = resolved.next().is_some_and(|at| at.ip().is_loopback());
    Ok(if local {
        Ipv4Addr::LOCALHOST
    } else {
        Ipv4Addr::UNSPECIFIED
    })
}

/// A peer number for a guest: anything but nobody and the host, and unlikely
/// to be another guest's.
pub(super) fn guest_id() -> PeerId {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let mixed = nanos ^ std::process::id().rotate_left(16);
    let low = u16::try_from(mixed % 65_000).unwrap_or(0);
    PeerId(low.saturating_add(2))
}
