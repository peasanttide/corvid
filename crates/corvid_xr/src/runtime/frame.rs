//! One frame: waiting for the compositor, reading the poses, and the
//! swapchain it is drawn into.
//!
//! The seam against `mod.rs` is the session's lifetime. Everything there
//! happens once -- an instance, a device, a session, an action set -- and
//! everything here happens ninety times a second.

use core::time::Duration;

use corvid_fixed::Angle16;

use crate::runtime::convert::{believed, lifecycle, pose, seen};
use crate::runtime::{Extent, OpenXr, STEREO, Swapchain, Unavailable};
use crate::{EyeView, Side, State, Tracked};

impl OpenXr {
    /// The frame stream, for the renderer that submits frames through it.
    ///
    /// This module acquires and releases swapchain images; beginning and ending
    /// a frame is the renderer's, because the composition layers it submits are
    /// built from what it drew.
    pub fn stream(&mut self) -> Option<&mut openxr::FrameStream<openxr::Vulkan>> {
        self.session.as_mut().map(|running| &mut running.stream)
    }

    /// Blocks until the compositor wants a frame, and reports when it intends
    /// to display it.
    ///
    /// The poses [`poll`](crate::Headset::poll) reads are located at that time rather
    /// than at now, which is what makes a headset feel attached to a head.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Runtime`] when the runtime refused, and
    /// [`Unavailable::NoHeadset`] when no session has been opened.
    pub fn wait_frame(&mut self) -> Result<Duration, Unavailable> {
        let running = self.session.as_mut().ok_or(Unavailable::NoHeadset)?;
        let frame = running.waiter.wait().map_err(Unavailable::from)?;
        self.predicted = frame.predicted_display_time;
        if self.began == 0 {
            self.began = self.predicted.as_nanos();
        }
        Ok(self.since_start(self.predicted))
    }

    /// The swapchain the runtime wants drawn into, as `wgpu` textures.
    ///
    /// One array texture with two layers when the device offers multiview, and
    /// two single-layer images when it does not.
    ///
    /// # Errors
    ///
    /// [`Unavailable::NoHeadset`] when no session has been opened,
    /// [`Unavailable::NoFormat`] when the runtime offers nothing `wgpu` reads,
    /// and [`Unavailable::Runtime`] when it refused.
    pub fn swapchain(&mut self, device: &wgpu::Device) -> Result<Swapchain, Unavailable> {
        self.open(device)?;
        let views = self
            .instance
            .enumerate_view_configuration_views(self.system, STEREO)
            .map_err(Unavailable::from)?;
        let first = views.first().ok_or(Unavailable::NoHeadset)?;
        let running = self.session.as_ref().ok_or(Unavailable::NoHeadset)?;
        Swapchain::open(
            &running.session,
            device,
            Extent {
                width: first.recommended_image_rect_width,
                height: first.recommended_image_rect_height,
            },
        )
    }

    /// A runtime time as a duration since the session began.
    pub(super) fn since_start(&self, time: openxr::Time) -> Duration {
        Duration::from_nanos((time.as_nanos() - self.began).max(0).unsigned_abs())
    }

    /// Reads the events the runtime has queued and moves the lifecycle.
    pub(super) fn drain(&mut self) {
        let mut events = openxr::EventDataBuffer::new();
        while let Ok(Some(event)) = self.instance.poll_event(&mut events) {
            match event {
                openxr::Event::SessionStateChanged(changed) => {
                    self.state = lifecycle(changed.state());
                    if self.began == 0 {
                        self.began = changed.time().as_nanos();
                    }
                }
                openxr::Event::InstanceLossPending(_) => self.state = State::Exiting,
                _ => {}
            }
        }
    }

    /// Locates the head, the eyes and the hands at the predicted display time.
    pub(super) fn locate(&mut self) {
        let Some(running) = self.session.as_ref() else {
            return;
        };
        if self.predicted.as_nanos() == 0 {
            return;
        }
        let at = self.since_start(self.predicted);

        if let Ok(location) = running.view.locate(&running.stage, self.predicted) {
            self.head = Tracked::new(pose(location.pose), believed(location.location_flags), at);
        }
        if let Ok((flags, eyes)) =
            running
                .session
                .locate_views(STEREO, self.predicted, &running.stage)
        {
            let confidence = seen(flags);
            let mut both = self.views.value;
            for (side, eye) in Side::ALL.iter().zip(&eyes) {
                let made = EyeView {
                    pose: pose(eye.pose),
                    left: Angle16::from_radians(f64::from(eye.fov.angle_left)),
                    right: Angle16::from_radians(f64::from(eye.fov.angle_right)),
                    up: Angle16::from_radians(f64::from(eye.fov.angle_up)),
                    down: Angle16::from_radians(f64::from(eye.fov.angle_down)),
                };
                match side {
                    Side::Left => both.left = made,
                    Side::Right => both.right = made,
                }
            }
            self.views = Tracked::new(both, confidence, at);
        }
        self.hands = Side::ALL.map(|side| running.actions.hand(running, side, self.predicted, at));
    }
}
