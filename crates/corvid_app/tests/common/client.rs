//! The controller and the ear these tests play with.
//!
//! The seam against `mod.rs` is the ring: everything there is the simulation,
//! which every peer agrees about, and everything here is one machine reading a
//! device and answering with a sound.

use corvid_behavior::{Extract, Extracting};
use corvid_control::{Acting, Controller, Updating};
use corvid_sound::{AudioFrame, Auralizer, Cue, Hearing, Listener, Source, SourceId};
use corvid_time::{Duration, Tick};
use corvid_vector::FinePoint;
use serde::{Deserialize, Serialize};

use super::{Action, CHIME, HUM, Tally, VOICE, action};

/// The player: what a tick's action is, and the client-local pause.
///
/// It holds the elapsed wall clock and the frame count, and it is the only
/// thing that writes them -- which is what `update` being the one `&mut self`
/// hook buys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hands {
    /// Simulated seconds handed to `update`.
    pub(crate) elapsed: Duration,
    /// How many displayed frames have gone by.
    pub(crate) frames: u64,
    /// Whether the simulation is held.
    pub(crate) paused: bool,
    /// How many frames it has been held for.
    pub(crate) held: u64,
    /// The tick and rules the last `update` saw, so `action` and `simulating`
    /// can read what only `update` is handed.
    pub(crate) at: Tick,
    /// What the pause is decided against.
    pub(crate) pause_at: Option<Tick>,
    /// And for how long.
    pub(crate) pause_for: u64,
}

/// What a `Hands` is built from: when to pause, and for how long.
///
/// A `Config` rather than something read off the simulation's own `Rules`,
/// which is the honest place for it: a pause is one machine's, and `Rules` is
/// what every peer agrees on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Holding {
    /// The tick from which to hold, if at all.
    pub(crate) pause_at: Option<Tick>,
    /// How many displayed frames to hold for.
    pub(crate) pause_for: u64,
}

impl Controller<Tally> for Hands {
    type Config = Holding;

    /// A fixture with nothing to press.
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(config: Holding) -> Self {
        Self {
            pause_at: config.pause_at,
            pause_for: config.pause_for,
            ..Self::default()
        }
    }

    fn configure(&mut self, config: Holding) {
        self.pause_at = config.pause_at;
        self.pause_for = config.pause_for;
    }

    /// Bump on one tick in three, counting the simulated seconds `update` has
    /// been handed as though they were ticks.
    ///
    /// The second term is what makes a wall clock visible. Under the app's fake
    /// clock it is a function of the tick number and nothing else, so the
    /// sequence is fixed; under a real clock a run of a few dozen ticks never
    /// reaches one second and the sequence is a different one.
    fn action(&self, acting: Acting<'_, Tally>) -> Action {
        if acting.input.digital(action::REST).held {
            return Action::Idle;
        }
        let phase = acting.state.now.0.wrapping_add(self.elapsed.as_secs());
        if phase.is_multiple_of(3) {
            Action::Bump
        } else {
            Action::Idle
        }
    }

    /// The one writer, and where this game's pause is decided.
    ///
    /// The pause is counted in *displayed frames* rather than in ticks, and it
    /// has to be: once it starts there are no more ticks, so a condition
    /// written against the tick number would never come round again.
    fn update(&mut self, updating: Updating<'_, Tally>) {
        self.elapsed = self.elapsed.saturating_add(updating.dt);
        self.frames += 1;
        self.at = updating.state.now;
        if self.pause_at.is_some_and(|at| updating.state.now >= at) {
            self.held = self.held.saturating_add(1);
            self.paused = self.held <= self.pause_for;
        }
    }

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }

    /// The client-local pause: no tick while this says so, while `update`,
    /// `hear` and the backend carry on.
    fn simulating(&self) -> bool {
        !self.paused
    }
}

/// The ear: one voice per unit of tally, and a chime on a tick that moved it.
///
/// The voice count varies with the state, which is what makes two ticks'
/// captured frames different files rather than the same bytes twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ears {
    /// The newest extracted state's count.
    count: i64,
    /// The one before it, so a change is noticeable.
    was: i64,
    /// Which tick the newest is.
    at: Tick,
}

impl Extract<Tally> for Ears {
    fn extract(&mut self, extracting: Extracting<'_, Tally>) {
        if extracting.state.now != self.at {
            self.was = self.count;
        }
        self.count = extracting.state.count;
        self.at = extracting.state.now;
    }
}

impl Auralizer<Tally> for Ears {
    type Config = ();

    fn new((): ()) -> Self {
        Self::default()
    }

    fn configure(&mut self, (): ()) {}

    fn hear(&mut self, out: &mut AudioFrame, hearing: Hearing) {
        out.listen(Listener::new(hearing.camera.pose));

        let voices = u32::try_from(self.count.rem_euclid(5)).unwrap_or(0) + 1;
        for voice in 0..voices {
            out.source(Source::new(SourceId(VOICE.0 + voice), HUM).at(FinePoint::ZERO));
        }

        if self.count != self.was {
            let id = out.next_id(self.at);
            out.cue(Cue::new(id, CHIME).at(FinePoint::ZERO));
        }
    }
}
