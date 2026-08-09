//! The curve one direction of one link follows, and the stream its draws come
//! from.

use core::time::Duration;

use corvid_fixed::Factor16;
use corvid_net::Link;
use rand::rngs::ChaCha8Rng;
use rand::{RngExt, SeedableRng};

/// The curve one direction of one link follows.
///
/// Directions are independent, because a real asymmetric link is the
/// interesting case: a peer whose uplink is the bad half is a peer whose
/// actions arrive late while everyone else's arrive on time.
///
/// ```
/// use corvid_net_mock::Schedule;
///
/// assert_eq!(Schedule::default(), Schedule::PERFECT);
/// assert_eq!(Schedule::MOBILE.latency.as_millis(), 120);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Schedule {
    /// The floor, which every datagram waits at least -- except one that
    /// [`reorder`](Self::reorder) moves ahead of a neighbour, which is the one
    /// draw that can beat it.
    pub latency: Duration,
    /// Added on top, uniform in `0 ..= jitter`, from the link's own stream.
    pub jitter: Duration,
    /// The share of datagrams dropped outright.
    pub loss: Factor16,
    /// The share of datagrams moved ahead of one in-flight neighbour.
    ///
    /// One neighbour, not the queue: a datagram lands a nanosecond before the
    /// latest one already in flight, and the mark it is measured against does
    /// not move backwards with it. Otherwise a link at
    /// [`ONE`](Factor16::ONE) hands back the whole burst reversed, which is
    /// not what a crossing is and not what
    /// [`Tally::reordered`](crate::Tally::reordered) counts.
    ///
    /// It is the only thing that reorders a link whose jitter is smaller than
    /// the interval between sends. Where jitter is the larger of the two,
    /// jitter already reorders and this adds to it -- so a curve with both, like
    /// [`MOBILE`](Self::MOBILE), owes most of its inversions to the jitter.
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

    /// 120 ms, 60 ms, 5 %, 5 %. A bad mobile one, and the curve a peer rolling
    /// back has to survive.
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
    /// use corvid_net_mock::Schedule;
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
    /// have taken, and never less than a millisecond -- a retransmit timer of
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
/// A [`ChaCha8Rng`] keyed by `(seed, link, sequence)` rather than a generator
/// carried between decisions: the stream one scheduling decision draws from is
/// a pure function of where it sits, never a system RNG and never the wall
/// clock. Two runs of the same script with the same seed draw the same numbers
/// in the same order, whatever else the process is doing -- and because nothing
/// is carried, a decision's draws do not depend on how many decisions came
/// before it on other links.
///
/// `ChaCha` because it is value-stable. [`StdRng`](rand::rngs::StdRng) is
/// explicitly allowed to change algorithm between `rand` releases, which for a
/// crate whose tests freeze delivery schedules as digests would be a silent
/// break on a routine bump.
pub(crate) struct Draws(ChaCha8Rng);

/// The number a link's draw stream is keyed by.
///
/// A local detail rather than something [`Link`] should answer: what makes two
/// directions draw apart is this crate's business, and the contract has no
/// opinion on how a backend keys anything.
pub(crate) fn key(link: Link) -> u64 {
    u64::from(link.from.to_u16()) << 16 | u64::from(link.to.to_u16())
}

impl Draws {
    /// The stream for one scheduling decision on one link.
    ///
    /// The three coordinates are laid into the key rather than hashed together
    /// first. `ChaCha`'s own permutation is the diffusion, so a structured key is
    /// no worse than a digest of one here, and it is one less thing between the
    /// seed a caller passes and the numbers it gets back.
    pub(crate) fn new(seed: u64, link: u64, sequence: u64) -> Self {
        let mut key = [0; 32];
        key[..8].copy_from_slice(&seed.to_le_bytes());
        key[8..16].copy_from_slice(&link.to_le_bytes());
        key[16..24].copy_from_slice(&sequence.to_le_bytes());
        Self(ChaCha8Rng::from_seed(key))
    }

    /// Whether an event of this share happens.
    ///
    /// Exact at both ends: a share of zero never happens and a share of one
    /// always does, which a naive comparison against a 16-bit draw gets wrong
    /// at whichever end its inequality is not strict on.
    pub(crate) fn hits(&mut self, share: Factor16) -> bool {
        let threshold = u64::from(share.to_bits()) * (1 << 32) / u64::from(u16::MAX);
        u64::from(self.0.random::<u32>()) < threshold
    }

    /// A duration uniform in `0 ..= span`, both ends included.
    ///
    /// `random_range` over an inclusive range, which is where the uniformity
    /// comes from. Scaling a draw by `span / u32::MAX` instead would make the
    /// top of the range a single draw out of four billion, so `spread(1ns)`
    /// would answer one nanosecond with probability two to the minus
    /// thirty-two rather than a half, and every span would be short by half a
    /// bucket.
    ///
    /// Built from seconds and nanoseconds rather than from a nanosecond count,
    /// because a span past about five hundred and eighty-four years does not
    /// fit one and would otherwise saturate to that.
    pub(crate) fn spread(&mut self, span: Duration) -> Duration {
        let nanos = self.0.random_range(0..=span.as_nanos());
        Duration::new(
            u64::try_from(nanos / 1_000_000_000).unwrap_or(u64::MAX),
            u32::try_from(nanos % 1_000_000_000).unwrap_or(0),
        )
    }
}
