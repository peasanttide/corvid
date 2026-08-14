//! The action set: the paths bound at start-up, and the poses read per frame.
//!
//! The seam is `OpenXR`'s own two-phase shape -- an action set is declared once
//! and suggested bindings are attached before a session starts, and after that
//! it is only ever sampled -- so nothing here touches a swapchain or a frame.

use core::time::Duration;

use crate::runtime::convert::{believed, factor, pose};
use crate::runtime::{Actions, Running, Unavailable};
use crate::{Confidence, Hand, Side, Tracked};

impl Actions {
    /// The one action set, bound to the profiles a controller answers to.
    pub(super) fn new(
        instance: &openxr::Instance,
        session: &openxr::Session<openxr::Vulkan>,
    ) -> Result<Self, Unavailable> {
        let set = instance
            .create_action_set("corvid", "Corvid", 0)
            .map_err(Unavailable::from)?;
        let hands = [
            instance
                .string_to_path("/user/hand/left")
                .map_err(Unavailable::from)?,
            instance
                .string_to_path("/user/hand/right")
                .map_err(Unavailable::from)?,
        ];
        let palm = set
            .create_action::<openxr::Posef>("palm", "Palm", &hands)
            .map_err(Unavailable::from)?;
        let aim = set
            .create_action::<openxr::Posef>("aim", "Aim", &hands)
            .map_err(Unavailable::from)?;
        let grip = set
            .create_action::<f32>("grip", "Grip", &hands)
            .map_err(Unavailable::from)?;
        let pinch = set
            .create_action::<f32>("pinch", "Pinch", &hands)
            .map_err(Unavailable::from)?;
        let rumble = set
            .create_action::<openxr::Haptic>("rumble", "Rumble", &hands)
            .map_err(Unavailable::from)?;

        // Every runtime knows the simple controller, and it has no analogue
        // inputs at all -- so the poses and the haptic are bound there, and the
        // grip and pinch to a profile that has them. A runtime that has never
        // heard of the second refuses the suggestion, which is why it is
        // allowed to fail.
        let mut bindings = Vec::new();
        for (index, hand) in ["/user/hand/left", "/user/hand/right"].iter().enumerate() {
            let path = |suffix: &str| instance.string_to_path(&format!("{hand}{suffix}"));
            if let (Ok(grip_pose), Ok(aim_pose), Ok(haptic)) = (
                path("/input/grip/pose"),
                path("/input/aim/pose"),
                path("/output/haptic"),
            ) {
                bindings.push(openxr::Binding::new(&palm, grip_pose));
                bindings.push(openxr::Binding::new(&aim, aim_pose));
                bindings.push(openxr::Binding::new(&rumble, haptic));
            }
            let _ = index;
        }
        if let Ok(profile) = instance.string_to_path("/interaction_profiles/khr/simple_controller")
        {
            let _ = instance.suggest_interaction_profile_bindings(profile, &bindings);
        }

        let mut analogue = bindings;
        for hand in ["/user/hand/left", "/user/hand/right"] {
            let path = |suffix: &str| instance.string_to_path(&format!("{hand}{suffix}"));
            if let (Ok(squeeze), Ok(trigger)) =
                (path("/input/squeeze/value"), path("/input/trigger/value"))
            {
                analogue.push(openxr::Binding::new(&grip, squeeze));
                analogue.push(openxr::Binding::new(&pinch, trigger));
            }
        }
        if let Ok(profile) =
            instance.string_to_path("/interaction_profiles/oculus/touch_controller")
        {
            let _ = instance.suggest_interaction_profile_bindings(profile, &analogue);
        }

        session
            .attach_action_sets(&[&set])
            .map_err(Unavailable::from)?;

        let space = |action: &openxr::Action<openxr::Posef>, hand: openxr::Path| {
            action
                .create_space(session, hand, openxr::Posef::IDENTITY)
                .map_err(Unavailable::from)
        };
        let palms = [space(&palm, hands[0])?, space(&palm, hands[1])?];
        let aims = [space(&aim, hands[0])?, space(&aim, hands[1])?];

        Ok(Self {
            set,
            grip,
            pinch,
            rumble,
            hands,
            palms,
            aims,
        })
    }

    /// One hand, located at `time`.
    pub(super) fn hand(
        &self,
        running: &Running,
        side: Side,
        time: openxr::Time,
        at: Duration,
    ) -> Tracked<Hand> {
        let index = side.index();
        let hand = self.hands[index];
        let palm = self.palms[index].locate(&running.stage, time);
        let aim = self.aims[index].locate(&running.stage, time);
        let (Ok(palm), Ok(aim)) = (palm, aim) else {
            return Tracked::new(Hand::default(), Confidence::Lost, at);
        };
        let closed = |action: &openxr::Action<f32>| {
            action.state(&running.session, hand).map_or(0.0, |state| {
                if state.is_active {
                    state.current_state
                } else {
                    0.0
                }
            })
        };
        Tracked::new(
            Hand {
                palm: pose(palm.pose),
                aim: pose(aim.pose),
                grip: factor(closed(&self.grip)),
                pinch: factor(closed(&self.pinch)),
            },
            believed(palm.location_flags),
            at,
        )
    }
}
