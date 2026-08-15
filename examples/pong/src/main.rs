//! Plays pong: alone, against a bot, or against another machine.
//!
//! ```text
//! pong                                       one seat, a window, nobody opposite
//! pong --bots 1                              a window, and an opponent
//! pong --headless --spectator --bots 2       two bots, no window, one digest
//! pong --listen 9000 --connect HOST:9001     two machines, over a socket
//! ```
//!
//! Everything else `corvid` accepts is accepted here -- `--ticks N`, `--seat N`,
//! `--record FILE`, `--demo FILE`, `--load N`, `--level NAME`, `--state DIR` --
//! because the whole of this binary is the declaration below and
//! [`corvid::main`] is what reads a command line.

use corvid::TickSpan;
use pong::{Ears, Graphics, Hands, Opponent, Table};

corvid::app! {
    /// Thirty-three milliseconds a tick -- a shade over thirty a second, and
    /// twice the workspace's fifteen-hertz default. Pong is a game of reacting
    /// to a ball, and a paddle that can only change direction twice in a tenth
    /// of a second reads as a paddle that is fighting you. It is also the more
    /// interesting rate for the netcode: at this period a domestic link's
    /// latency is two or three ticks rather than one, so prediction has
    /// something to do.
    ///
    /// A round number of milliseconds rather than a round number of hertz,
    /// because a period is what a game is asked for and what every peer has to
    /// agree on: thirty hertz is 33 333 333 nanoseconds, which is a number
    /// nobody types the same way twice.
    struct Pong;
    const PERIOD: TickSpan = TickSpan::from_millis(33);
    type State = Table;
    type Controller = Hands;
    type Bot = Opponent;
    type Render = Graphics;
    type Auralizer = Ears;
}
