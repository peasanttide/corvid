//! The discard schedule a `dev` build used to simulate a fresh scratch on.
//!
//! **Nothing consults this any more.** `State` carried a `Scratch` associated
//! type and a `dev` build periodically replaced the accumulated one before a
//! tick, so that a game leaning on what its pools happened to still hold failed
//! in development rather than on somebody else's machine. There is no `Scratch`
//! on the contract now, so there is nothing to discard and no caller: what is
//! left is the schedule itself, which is still a pure function of the session
//! and still exact across machines.

use corvid_hash::Hasher;
use corvid_replay::Seed;
use corvid_time::Tick;

/// One tick in this many is on the schedule.
///
/// Four rather than one, because discarding every tick would have made a game's
/// pools useless and would have tested the empty scratch and nothing else; and
/// rather than a thousand, because a schedule that fires once a minute is a
/// schedule nobody's test run reaches. At fifteen ticks a second this fires
/// about four times a second, so a leak would show up inside the first second
/// of play.
pub const ONE_TICK_IN: u64 = 4;

/// Whether a `dev` build replaces the accumulated scratch before simulating
/// `tick`.
///
/// The schedule is a function of the session's [`Seed`] and the tick number and
/// of nothing else — not of the wall clock, not of the machine, not of how much
/// memory this peer has. That is what "part of the session" means and what it
/// buys: two `dev` peers playing the same session discard on exactly the same
/// ticks and agree with each other, so a `dev` build is a build a team can
/// play together on.
///
/// It is folded through the digest rather than being `tick % 4` so that a game
/// whose own behaviour has a period cannot line up with it. A game that bumped
/// a counter every fourth tick and a schedule that discarded every fourth tick
/// would be a schedule that only ever discarded before the same kind of tick.
///
/// ```
/// # #[cfg(feature = "dev")] {
/// use corvid_app::dev;
/// use corvid_replay::Seed;
/// use corvid_time::Tick;
///
/// // The same session discards on the same ticks, every run, on every machine.
/// assert_eq!(dev::discards(Seed(7), Tick(3)), dev::discards(Seed(7), Tick(3)));
///
/// // Two sessions with different seeds do not discard together, which is why
/// // one game's schedule says nothing about another's.
/// let seven: Vec<bool> = (0..64).map(|t| dev::discards(Seed(7), Tick(t))).collect();
/// let eight: Vec<bool> = (0..64).map(|t| dev::discards(Seed(8), Tick(t))).collect();
/// assert_ne!(seven, eight);
/// # }
/// ```
#[must_use]
pub const fn discards(seed: Seed, tick: Tick) -> bool {
    Hasher::new()
        .absorb(seed.0)
        .absorb(tick.0)
        .digest()
        .to_u64()
        .is_multiple_of(ONE_TICK_IN)
}

/// The first tick at or after `from` that a `dev` build discards on, searching
/// no further than `until`.
///
/// A convenience for a test or a tool that wants to say *when* the schedule
/// will next fire. It is a search rather than arithmetic because the schedule
/// is a digest and has no closed form.
#[must_use]
pub fn next_discard(seed: Seed, from: Tick, until: Tick) -> Option<Tick> {
    let mut tick = from;
    while tick < until {
        if discards(seed, tick) {
            return Some(tick);
        }
        tick = tick.next();
    }
    None
}
