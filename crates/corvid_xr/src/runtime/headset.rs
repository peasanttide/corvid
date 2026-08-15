//! The [`Headset`] contract, as a real runtime answers it.
//!
//! The seam against `mod.rs` is the trait: everything here is a method a game
//! calls through the contract, and everything there is the session machinery
//! those methods read.

use crate::runtime::convert::{believed, pose};
use crate::runtime::{ASSUMED_RATE, OpenXr, STEREO};
use crate::{Hand, Haptic, Headset, Passthrough, Pose, Side, Space, State, Tracked, Views};

impl Headset for OpenXr {
    fn poll(&mut self) -> State {
        self.drain();
        if let Some(running) = self.session.as_ref() {
            match self.state {
                State::Ready => {
                    let _ = running.session.begin(STEREO);
                }
                State::Stopping => {
                    let _ = running.session.end();
                }
                _ => {}
            }
            if self.state.is_drawing() {
                let _ = running
                    .session
                    .sync_actions(&[openxr::ActiveActionSet::new(&running.actions.set)]);
                self.locate();
            }
        }
        self.state
    }

    fn head(&self, space: Space) -> Tracked<Pose> {
        let Some(running) = self.session.as_ref() else {
            return self.head;
        };
        if self.predicted.as_nanos() == 0 {
            return self.head;
        }
        let base = match space {
            Space::Stage => &running.stage,
            Space::Local => &running.local,
            Space::View => &running.view,
        };
        running
            .view
            .locate(base, self.predicted)
            .map_or(self.head, |location| {
                Tracked::new(
                    pose(location.pose),
                    believed(location.location_flags),
                    self.since_start(self.predicted),
                )
            })
    }

    fn views(&self) -> Tracked<Views> {
        self.views
    }

    fn hands(&self) -> [Tracked<Hand>; 2] {
        self.hands
    }

    fn rumble(&mut self, hand: usize, effect: Haptic) {
        let Ok(side) = Side::try_from(hand) else {
            return;
        };
        let Some(running) = self.session.as_ref() else {
            return;
        };
        let event = openxr::HapticVibration::new()
            .amplitude(effect.amplitude.to_f32())
            .frequency(f32::from(effect.frequency))
            .duration(openxr::Duration::from_nanos(
                i64::try_from(effect.duration.as_nanos()).unwrap_or(i64::MAX),
            ));
        let _ = running.actions.rumble.apply_feedback(
            &running.session,
            running.actions.hands[side.index()],
            &event,
        );
    }

    fn passthrough(&self) -> Passthrough {
        self.passthrough
    }

    fn set_passthrough(&mut self, on: bool) -> Passthrough {
        // Which blend mode a frame is submitted with is the renderer's, and it
        // reads this. Refusing on a headset that cannot show the room is the
        // normal answer rather than a failure.
        self.passthrough = self.passthrough.asked(on);
        self.passthrough
    }

    fn rate(&self) -> u16 {
        ASSUMED_RATE
    }
}
