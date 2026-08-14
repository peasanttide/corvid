//! The controller, the openings and the game these checks are pointed at.
//!
//! The seam against `mod.rs` is the ring: everything there is the simulation
//! -- the level, the rules, the state -- and everything here is the client
//! half and the assembly.

use std::sync::Arc;

use corvid_behavior::ProfileId;
use corvid_hash::Digest;
use corvid_input::Input;
use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
use corvid_time::Tick;

use super::{Cliff, Climb, Habit, Rules, Step, spin};

/// The climber: what a tick's action is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Legs {
    /// The rules this controller was built with, since only the simulation is
    /// handed them now.
    pub(crate) rules: Rules,
}

impl corvid_control::Controller<Climb> for Legs {
    type Config = Rules;

    /// A fixture with nothing to press.
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(rules: Rules) -> Self {
        Self { rules }
    }

    fn configure(&mut self, rules: Rules) {
        self.rules = rules;
    }

    fn action(&self, _acting: corvid_control::Acting<'_, Climb>) -> Step {
        if matches!(self.rules.habit, Habit::Fickle)
            && spin(self.rules.spin) >= self.rules.threshold
        {
            Step::Leap
        } else {
            Step::Up
        }
    }

    fn update(&mut self, _updating: corvid_control::Updating<'_, Climb>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

/// The level every opening here is on.
pub(crate) const CLIFF: &str = "cliff";

/// How this build describes its own types.
#[must_use]
pub(crate) fn schema() -> Digest {
    Schema::new("wobble")
        .field("Climb.metres", "i64")
        .field("Climb.now", "Tick")
        .digest()
}

/// An opening with the given habit, counter and threshold, on a level and an
/// origin that both survive being written down.
#[must_use]
pub(crate) const fn rules(habit: Habit, spin: u8, threshold: i64) -> Rules {
    Rules {
        habit,
        spin,
        threshold,
    }
}

/// The opening every run here plays from.
#[must_use]
pub(crate) fn opening(habit: Habit, spin: u8, threshold: i64) -> Opening<Climb> {
    Opening {
        level: CLIFF.to_owned(),
        content: Arc::new(Cliff { rise: 1, hidden: 0 }),
        rules: Arc::new(Rules {
            habit,
            spin,
            threshold,
        }),
        roster: vec![Profile {
            account: ProfileId(1),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(7),
        first: Tick::ZERO,
        origin: None,
        schema: schema(),
    }
}

/// A [`Habit::Steady`] opening whose level carries a field a capture does not
/// record.
///
/// Its first tick is right and every one after it is wrong, because the level is
/// what came back rather than what went down.
#[must_use]
pub(crate) fn opening_on_a_lossy_level() -> Opening<Climb> {
    let mut opening = opening(Habit::Steady, 0, 0);
    opening.content = Arc::new(Cliff { rise: 1, hidden: 4 });
    opening
}

/// A [`Habit::Steady`] opening whose *origin* carries a field a capture does not
/// record.
///
/// Its very first mark is wrong, because the state a replay starts from is not
/// the state the run started from.
#[must_use]
pub(crate) fn opening_with_a_lossy_origin() -> Opening<Climb> {
    let mut opening = opening(Habit::Steady, 0, 0);
    // A whole new handle rather than a write through the old one. An `Arc` has
    // no `DerefMut`, so an opening's origin is replaced and never edited --
    // which is the same shape the level above it already had.
    opening.origin = Some(Arc::new(Climb {
        unwritten: 7,
        ..Climb::default()
    }));
    opening
}

/// Where a run of the climb that was told nothing else starts.
///
/// Every check here says which opening it wants, because what these fixtures
/// vary is the habit: which global a tick reads and when it starts reading it.
/// This is here because a [`Game`](corvid_app::Game) names a state that can
/// open a session on its own, and the steady habit is the honest answer for a
/// run nobody has configured.
impl Opens for Climb {
    fn opening() -> Opening<Self> {
        opening(Habit::Steady, 0, 0)
    }
}

/// The game the replay checks play: the climb, with nobody at the controls.
///
/// Four `()`s. What a replay check compares is a session against the states it
/// recorded, and neither a controller nor a device is on the path between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Climbing;

impl corvid_app::Game for Climbing {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Climb;
    type Controller = ();
    type Bot = ();
    type Render = ();
    type Auralizer = ();
}

/// The input every run here plays with: nothing held, because this game's
/// `action` does not read one.
#[must_use]
pub(crate) fn idle() -> Input {
    Input::new(&[])
}
