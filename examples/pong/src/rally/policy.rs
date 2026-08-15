//! The seat that is not a person: one scripted control, two arms.
//!
//! The seam against `mod.rs` is what is being measured against what is doing
//! the measuring. A [`Match`](crate::rally::Match) owns peers, a link and a
//! trace; this is only what answers with an action when one of those peers asks
//! a seat what it is doing.

use corvid::Controller;
use serde::{Deserialize, Serialize};

use crate::{Move, Table};

/// What a seat's player does, tick by tick.
///
/// Two so far, and they are the two a netcode test needs: one that watches the
/// ball, so the peers disagree about the future often enough for prediction to
/// be worth testing, and one that does nothing, so a seat can be present and
/// idle.
///
/// A [`Controller`] rather than a function pointer, because that is what it
/// stands in for: the lab drives a [`Peer`](corvid_lockstep::Peer) directly
/// where a run drives it through [`App`](corvid::App), and the thing being
/// substituted either way is the control that answers with an action per tick.
/// As a real controller it
/// can be handed to an `App` unchanged, which is what makes the lab and a run
/// the same setup rather than two shapes that have to be kept in step.
///
/// **Neither arm is written here.** [`Chase`](Self::Chase) is
/// [`Opponent`](crate::Opponent) and [`Idle`](Self::Idle) is `()`, so what the
/// lab measures is the paddle a player is actually played against and the
/// silence a seat nobody is in actually submits. Which paddle is which seat's is
/// [`Acting::seat`](corvid::Acting), so a policy carries no seat of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Policy {
    /// Works out where the ball is going, goes there, and decides which part of
    /// the paddle to meet it with. [`Opponent`](crate::Opponent) is the whole of
    /// it and carries the argument.
    #[default]
    Chase,
    /// Stands still forever, which is the shape of a seat nobody is sitting in.
    Idle,
}

impl Controller<Table> for Policy {
    /// Itself: which of the two it is, is the whole of what one is.
    type Config = Self;

    /// No device on either arm, so nothing to declare. The lab hands every
    /// policy the same empty snapshot and neither of them reads it.
    const REAL: bool = false;
    const SETS: &'static [corvid::SetDescriptor] = &[];

    fn new(config: Self) -> Self {
        config
    }

    fn configure(&mut self, config: Self) {
        *self = config;
    }

    /// The input is ignored, which is the point: a scripted seat answers from
    /// the state rather than from a device.
    fn action(&self, acting: corvid::Acting<'_, Table>) -> Move {
        match self {
            Self::Idle => ().action(acting),
            Self::Chase => crate::Opponent.action(acting),
        }
    }

    /// Nothing accumulates: there is no camera to smooth and no cursor to cast.
    fn update(&mut self, _updating: corvid::Updating<'_, Table>) {}

    fn look(&self) -> corvid::Camera {
        corvid::Camera::default()
    }
}
