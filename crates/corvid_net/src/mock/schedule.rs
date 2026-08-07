//! The curve one direction of one link follows, and the stream its draws come
//! from.

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_hash::Hasher;

/// The curve one direction of one link follows.
///
/// Directions are independent, because a real asymmetric link is the
/// interesting case: a peer whose uplink is the bad half is a peer whose
/// actions arrive late while everyone else's arrive on time.
///
/// ```
/// use corvid_net::Schedule;
///
/// assert_eq!(Schedule::default(), Schedule::PERFECT);
/// assert_eq!(Schedule::MOBILE.latency.as_millis(), 120);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Schedule {
    /// The floor. Every datagram waits at least this long.
    pub latency: Duration,
    /// Added on top, uniform in `0 ..= jitter`, from the link's own stream.
    pub jitter: Duration,
    /// The share of datagrams dropped outright.
    pub loss: Factor16,
    /// The share of datagrams whose delivery instant is moved across an
    /// in-flight neighbour's, which is what actually produces an out-of-order
    /// arrival.
    pub reorder: Factor16,
}

impl Schedule {
    /// No latency, no jitter, no loss, no reorder. What a determinism test
    /// wants, and what every link of a fresh network follows.
    pub const PERFECT: Self = Self::new(
        Duration::ZERO,
        Duration::ZERO,
        Factor16::ZERO,
        Factor16::ZERO,
    );

    /// 40 ms, 10 ms, 1 %, 1 %. A good domestic connection.
    pub const DOMESTIC: Self = Self::new(
        Duration::from_millis(40),
        Duration::from_millis(10),
        Factor16::from_f64(0.01),
        Factor16::from_f64(0.01),
    );

    /// 120 ms, 60 ms, 5 %, 5 %. A bad mobile one, and the curve the rollback
    /// budget is argued against.
    pub const MOBILE: Self = Self::new(
        Duration::from_millis(120),
        Duration::from_millis(60),
        Factor16::from_f64(0.05),
        Factor16::from_f64(0.05),
    );

    /// A curve of your own.
    ///
    /// ```
    /// use core::time::Duration;
    ///
    /// use corvid_fixed::Factor16;
    /// use corvid_net::Schedule;
    ///
    /// // A satellite hop: far away, steady, and clean.
    /// let via_orbit = Schedule::new(
    ///     Duration::from_millis(600),
    ///     Duration::from_millis(20),
    ///     Factor16::from_f64(0.001),
    ///     Factor16::ZERO,
    /// );
    ///
    /// assert_eq!(via_orbit.reorder, Factor16::ZERO);
    /// ```
    #[must_use]
    pub const fn new(
        latency: Duration,
        jitter: Duration,
        loss: Factor16,
        reorder: Factor16,
    ) -> Self {
        Self {
            latency,
            jitter,
            loss,
            reorder,
        }
    }

    /// How long a lost stream frame waits before it is tried again.
    ///
    /// Twice the latency, which is the round trip an acknowledgement would
    /// have taken, and never less than a millisecond — a retransmit timer of
    /// zero is not a timer, and a link with no latency at all would otherwise
    /// retry inside the instant it failed in.
    pub(crate) const fn retransmit(self) -> Duration {
        let doubled = self.latency.saturating_mul(2);
        let floor = Duration::from_millis(1);
        if doubled.as_nanos() > floor.as_nanos() {
            doubled
        } else {
            floor
        }
    }
}

/// One link's draw stream.
///
/// Counter-based rather than a generator carrying state: a draw is a pure
/// function of `(seed, link, sequence, counter)`, never a system RNG and never
/// the wall clock. Two runs of the same script with the same seed draw the
/// same numbers in the same order, whatever else the process is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Rng {
    seed: u64,
    link: u64,
    sequence: u64,
    counter: u64,
}

impl Rng {
    /// The stream for one scheduling decision on one link.
    pub(crate) const fn new(seed: u64, link: u64, sequence: u64) -> Self {
        Self {
            seed,
            link,
            sequence,
            counter: 0,
        }
    }

    /// The next 32 bits, widened.
    const fn next(&mut self) -> u64 {
        let digest = Hasher::with_seed(self.seed)
            .absorb(self.link)
            .absorb(self.sequence)
            .absorb(self.counter)
            .digest();
        self.counter += 1;
        digest.to_u64() >> 32
    }

    /// Whether an event of this share happens.
    ///
    /// Exact at both ends: a share of zero never happens and a share of one
    /// always does, which a naive comparison against a 16-bit draw gets wrong
    /// at whichever end its inequality is not strict on.
    pub(crate) fn hits(&mut self, share: Factor16) -> bool {
        let threshold = u64::from(share.to_bits()) * (1 << 32) / u64::from(u16::MAX);
        self.next() < threshold
    }

    /// A duration uniform in `0 ..= span`, both ends included.
    pub(crate) fn spread(&mut self, span: Duration) -> Duration {
        let drawn = u128::from(self.next());
        let nanos = span.as_nanos() * drawn / u128::from(u32::MAX);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}
