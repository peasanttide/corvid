//! What a run has added up to.

/// What has happened, for a lab's graph and a test's assertion.
///
/// `sent` counts datagrams and stream frames the network accepted; `delivered`
/// counts the ones a sink has seen or still can. A join or a loss is a
/// connection event rather than traffic and is in neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tally {
    /// Datagrams and stream frames handed to the network.
    pub sent: u64,
    /// Datagrams and stream frames a poll has handed over or still will.
    pub delivered: u64,
    /// Datagrams lost outright, plus stream attempts that were lost and
    /// retried.
    pub dropped: u64,
    /// Datagrams whose delivery instant was moved across an in-flight
    /// neighbour's.
    pub reordered: u64,
    /// How much is waiting to be delivered right now.
    pub in_flight: u32,
}
