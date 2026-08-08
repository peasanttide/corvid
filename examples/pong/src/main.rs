//! Plays pong: alone, against a bot, or against another machine.
//!
//! ```text
//! pong                                       one seat, a window, nobody opposite
//! pong --bots 1                              a window, and an opponent
//! pong --headless --spectator --bots 2       two bots, no window, one digest
//! pong --listen 9000 --connect HOST:9001     two machines, over a socket
//! ```
//!
//! Everything else `corvid` accepts is accepted here — `--ticks N`, `--seat N`,
//! `--record FILE`, `--demo FILE`, `--load N`, `--level JSON`, `--state DIR` —
//! because the whole of this binary is the declaration below and
//! [`corvid::main`] is what reads a command line.

use corvid::TickSpan;
use pong::{Ears, Graphics, Hands, Opponent, Table};

corvid::app! {
    /// Thirty-three milliseconds a tick, which is thirty a second and not the
    /// workspace's fifteen. Pong is a game of reacting to a ball, and a paddle
    /// that can only change direction twice in a tenth of a second reads as a
    /// paddle that is fighting you. It is also the more interesting rate for
    /// the netcode: at this period a domestic link's latency is two or three
    /// ticks rather than one, so prediction has something to do.
    struct Pong;
    const PERIOD: TickSpan = TickSpan::from_millis(33);
    type State = Table;
    type Controller = Hands;
    type Bot = Opponent;
    type Render = Graphics;
    type Auralizer = Ears;
}
