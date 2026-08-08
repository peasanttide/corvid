#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! The split between the one method that writes and the three that read.

use core::time::Duration;

use corvid_behavior::{Level, PlayerId, State, Time};
use corvid_camera::Camera;
use corvid_control::{Acting, Controller, Updating};
use corvid_files::{Malformed, Source};
use corvid_input::{Input, SetDescriptor};
use corvid_rotation::FineRotation;
use corvid_shape::Frustum;
use corvid_transform::GlobalFineTransform;
use corvid_vector::globalfinepoint;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Field;

impl Level for Field {
    type Reference = String;
    fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> {
        Ok(Self)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Walk;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Step(bool);

impl State for Walk {
    const NAME: &'static str = "walk";
    type Rules = ();
    type Level = Field;
    type Action = Step;
}

/// A controller whose camera climbs, and only in `update`.
#[derive(Debug, Default)]
struct Hands {
    /// Millimetres up, accumulated across displayed frames.
    height: i32,
    /// What `configure` was last told, so that reconfiguring is observable.
    speed: i32,
}

/// How fast this controller climbs, per second.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Speed(i32);

impl Controller<Walk> for Hands {
    type Config = Speed;
    const SETS: &'static [SetDescriptor] = &[];

    fn new(config: Speed) -> Self {
        Self {
            height: 0,
            speed: config.0,
        }
    }

    /// Kept rather than rebuilt, which is what a controller holding a camera
    /// owes: the default `configure` would throw the height away.
    fn configure(&mut self, config: Speed) {
        self.speed = config.0;
    }

    fn update(&mut self, updating: Updating<'_, Walk>) {
        self.height += self.speed * i32::try_from(updating.dt.as_millis()).unwrap_or(i32::MAX);
    }

    fn look(&self) -> Camera {
        Camera::new(
            GlobalFineTransform::new(globalfinepoint(0, 0, self.height), FineRotation::IDENTITY),
            Frustum::default(),
        )
    }

    fn action(&self, _acting: Acting<'_, Walk>) -> Step {
        Step(true)
    }
}

fn frame(hands: &mut Hands, millis: u64) {
    hands.update(Updating {
        state: &Walk,
        input: &Input::new(&[]),
        loading: None,
        time: Time::default(),
        dt: Duration::from_millis(millis),
        seat: PlayerId(0),
    });
}

#[test]
fn the_camera_moves_in_update_and_look_only_reports_it() {
    let mut hands = Hands::new(Speed(1));
    let before = hands.look();

    frame(&mut hands, 16);
    assert_ne!(hands.look(), before, "update moved it");

    // Twice, because the claim is that reading is free of side effects rather
    // than that one read happens to agree with itself.
    let after = hands.look();
    assert_eq!(hands.look(), after, "look moves nothing");
    assert_eq!(hands.look(), after);
}

#[test]
fn a_frame_that_saw_no_time_moves_nothing() {
    let mut hands = Hands::new(Speed(1));
    frame(&mut hands, 0);
    assert_eq!(hands.look(), Hands::new(Speed(1)).look());
}

/// Reconfiguring keeps what the controller had accumulated.
///
/// The default `configure` rebuilds, which is right for a controller holding
/// nothing and wrong for one holding a camera — a settings slider must not
/// teleport the view.
#[test]
fn configuring_a_running_controller_keeps_its_camera() {
    let mut hands = Hands::new(Speed(1));
    frame(&mut hands, 100);
    let climbed = hands.look();

    hands.configure(Speed(4));
    assert_eq!(hands.look(), climbed, "the slider moved the camera");

    frame(&mut hands, 100);
    assert_ne!(hands.look(), climbed, "and the new speed took effect");
}

#[test]
fn the_unit_controller_is_not_real_and_answers_the_idle_action() {
    // Read through a function so the assertion is about the trait rather than
    // about a constant clippy can fold away.
    fn real<S: State, C: Controller<S>>() -> bool {
        C::REAL
    }
    fn sets<S: State, C: Controller<S>>() -> &'static [SetDescriptor] {
        C::SETS
    }

    assert!(!real::<Walk, ()>(), "a dedicated server opens no window");
    assert!(sets::<Walk, ()>().is_empty());

    let mut nobody: () = Controller::<Walk>::new(());
    // A dropped player submits the default forever, and so does this: a seat
    // driven by `()` is a seat nobody is sitting in.
    assert_eq!(
        nobody.action(Acting {
            state: &Walk,
            input: &Input::new(&[]),
            time: Time::default(),
            seat: PlayerId(0),
        }),
        Step::default(),
    );
    assert_eq!(
        Controller::<Walk>::look(&nobody).pose,
        GlobalFineTransform::IDENTITY,
    );

    // And it survives being driven, which is what makes it usable as a default
    // rather than as a placeholder that panics.
    Controller::<Walk>::update(
        &mut nobody,
        Updating {
            state: &Walk,
            input: &Input::new(&[]),
            loading: None,
            time: Time::default(),
            dt: Duration::from_millis(16),
            seat: PlayerId(0),
        },
    );
}

#[test]
fn a_real_controller_is_real_by_default() {
    fn real<S: State, C: Controller<S>>() -> bool {
        C::REAL
    }
    assert!(real::<Walk, Hands>());
}

#[test]
fn a_controller_is_told_which_seat_it_answers_for() {
    let hands = Hands::new(Speed(0));
    let state = Walk;
    let input = Input::new(&[]);
    let time = Time::default();

    let first = hands.action(Acting {
        state: &state,
        input: &input,
        time,
        seat: PlayerId(0),
    });
    let second = hands.action(Acting {
        state: &state,
        input: &input,
        time,
        seat: PlayerId(1),
    });

    // `Hands` here ignores the seat, so both answers are the idle one — what
    // this pins is that the seat reaches the call at all.
    assert_eq!(first, second);
}
