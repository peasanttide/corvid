//! A real headset, through `OpenXR`. Behind the `openxr` feature.
//!
//! Everything a game touches is the same vocabulary the stand-in speaks: this
//! module implements [`Headset`] a second time, and nothing above it knows
//! which one it is holding.
//!
//! The loader is resolved at run time rather than linked, so building this
//! feature needs no SDK and running it on a machine with no runtime answers
//! [`Unavailable::NoRuntime`] rather than failing to start.
//!
//! # What this module does and does not do
//!
//! It does the session lifecycle, the reference spaces, the view configuration,
//! the controller poses and their grip and trigger, haptics, whether
//! passthrough is on offer, and the swapchain — including handing back the
//! per-eye images as `wgpu` textures, which is the part that needs the Vulkan
//! interop.
//!
//! It does **not** submit frames. `xrBeginFrame`, the composition layers and
//! `xrEndFrame` belong with the renderer's XR target, because they are where
//! the drawing is; [`Swapchain::acquire`] and [`Swapchain::release`] are the
//! two ends this module offers it.
//!
//! # What only a headset can find
//!
//! None of this is exercised by the scripted headset, and none of it runs in
//! CI. A session that fails to create, a swapchain format the runtime does not
//! offer, a frame the compositor drops — the stand-in stops these paths from
//! rotting, and a headset in somebody's hands is the only thing that certifies
//! them.

#![allow(
    clippy::cast_possible_truncation,
    reason = "the runtime speaks f32 and this module narrows fixed-point values into it at the boundary; nothing downstream of that is hashed, sent or replayed"
)]

mod vulkan;

use core::time::Duration;

use corvid_fixed::{Angle16, I2F30, I48F16};

use corvid_rotation::{FineRotation, Versor};

use crate::{
    Confidence, EyeView, Hand, Haptic, Headset, Passthrough, Pose, Side, Space, State, Tracked,
    Views,
};
use corvid_vector::GlobalFinePoint;

/// The view configuration a headset uses: two eyes.
const STEREO: openxr::ViewConfigurationType = openxr::ViewConfigurationType::PRIMARY_STEREO;

/// How many frames a second to report when the runtime will not say.
///
/// `XR_FB_display_refresh_rate` is the extension that answers this properly and
/// most runtimes do not offer it; ninety is what a consumer headset runs at and
/// is what a game's fixed step is usually built around.
const ASSUMED_RATE: u16 = 90;

/// The longest to wait for the compositor to finish with a swapchain image:
/// a tenth of a second, which is nine frames at ninety.
const PATIENCE: i64 = 100_000_000;

/// Why a headset is not there.
///
/// A reason a person can act on rather than a number, except in the one case
/// where the runtime gave a number and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unavailable {
    /// No `OpenXR` runtime is installed, or the one that is does not do Vulkan.
    NoRuntime,
    /// One is, and it has no headset attached.
    NoHeadset,
    /// The runtime does not offer a format `wgpu` can use.
    NoFormat,
    /// The Vulkan device `OpenXR` wants is not the one `wgpu` created.
    ///
    /// Also what a device on another backend answers: a Metal or DX12 device
    /// has no Vulkan handles to hand over.
    DeviceMismatch,
    /// The runtime said no, with its own code.
    Runtime(i32),
}

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRuntime => f.write_str("no OpenXR runtime with Vulkan support is installed"),
            Self::NoHeadset => f.write_str("an OpenXR runtime is installed, with no headset"),
            Self::NoFormat => f.write_str("the runtime offers no swapchain format wgpu can use"),
            Self::DeviceMismatch => {
                f.write_str("the graphics device the runtime wants is not the one wgpu created")
            }
            Self::Runtime(code) => write!(f, "the runtime refused with code {code}"),
        }
    }
}

impl core::error::Error for Unavailable {}

impl From<openxr::sys::Result> for Unavailable {
    fn from(result: openxr::sys::Result) -> Self {
        match result {
            openxr::sys::Result::ERROR_FORM_FACTOR_UNAVAILABLE
            | openxr::sys::Result::ERROR_FORM_FACTOR_UNSUPPORTED => Self::NoHeadset,
            openxr::sys::Result::ERROR_SWAPCHAIN_FORMAT_UNSUPPORTED => Self::NoFormat,
            other => Self::Runtime(other.into_raw()),
        }
    }
}

/// A real headset, through `OpenXR`.
///
/// Created without a graphics device, so a game can ask whether there is a
/// headset before it has opened one. [`open`](Self::open) is what binds it to a
/// device and starts a session.
pub struct OpenXr {
    /// Kept alive because the instance's entry points live in it.
    _entry: openxr::Entry,
    /// The runtime.
    instance: openxr::Instance,
    /// The headset it found.
    system: openxr::SystemId,
    /// What the lifecycle is doing.
    state: State,
    /// The session, once a device has been handed over.
    session: Option<Running>,
    /// The last frame time the runtime predicted, in its own clock.
    predicted: openxr::Time,
    /// When the session began, so a reading's `at` is since then.
    began: i64,
    /// Whether the room can be shown, and whether it is being.
    passthrough: Passthrough,
    /// The last head reading.
    head: Tracked<Pose>,
    /// The last view reading.
    views: Tracked<Views>,
    /// The last hand readings.
    hands: [Tracked<Hand>; 2],
}

/// Everything that exists only once a session does.
struct Running {
    /// The session.
    session: openxr::Session<openxr::Vulkan>,
    /// What blocks until the compositor wants a frame.
    waiter: openxr::FrameWaiter,
    /// What frames are submitted through, by the renderer rather than here.
    stream: openxr::FrameStream<openxr::Vulkan>,
    /// The floor under the player.
    stage: openxr::Space,
    /// Where the head was when the session began.
    local: openxr::Space,
    /// The head itself.
    view: openxr::Space,
    /// The inputs.
    actions: Actions,
}

/// The action set, its actions, and the spaces the pose actions denote.
struct Actions {
    /// The one set.
    set: openxr::ActionSet,
    /// How closed each fist is.
    grip: openxr::Action<f32>,
    /// How closed each pinch is.
    pinch: openxr::Action<f32>,
    /// Rumble.
    rumble: openxr::Action<openxr::Haptic>,
    /// `/user/hand/left` and `/user/hand/right`.
    hands: [openxr::Path; 2],
    /// Where each hand is.
    palms: [openxr::Space; 2],
    /// Where each hand points.
    aims: [openxr::Space; 2],
}

impl OpenXr {
    /// Whether a runtime is installed at all, without creating anything.
    ///
    /// Answers `false` on a machine with none, and does so without allocating a
    /// session, printing, or failing.
    #[must_use]
    pub fn available() -> bool {
        vulkan::entry().is_ok_and(|entry| {
            entry.enumerate_extensions().is_ok_and(|extensions| {
                extensions.khr_vulkan_enable2 || extensions.khr_vulkan_enable
            })
        })
    }

    /// Finds the runtime and the headset attached to it.
    ///
    /// No graphics device is needed yet, and no session is created:
    /// [`open`](Self::open) is what does that.
    ///
    /// # Errors
    ///
    /// [`Unavailable::NoRuntime`] when there is no runtime or it cannot do
    /// Vulkan, [`Unavailable::NoHeadset`] when there is a runtime and no
    /// headset, and [`Unavailable::Runtime`] when it refused for its own
    /// reasons.
    pub fn new(application: &str) -> Result<Self, Unavailable> {
        let entry = vulkan::entry()?;
        let offered = entry.enumerate_extensions().map_err(Unavailable::from)?;
        if !offered.khr_vulkan_enable2 && !offered.khr_vulkan_enable {
            return Err(Unavailable::NoRuntime);
        }
        let mut wanted = openxr::ExtensionSet::default();
        wanted.khr_vulkan_enable2 = offered.khr_vulkan_enable2;
        wanted.khr_vulkan_enable = !offered.khr_vulkan_enable2 && offered.khr_vulkan_enable;

        let info = openxr::ApplicationInfo {
            // The runtime's field is a fixed-width buffer and the crate refuses
            // anything longer, so this is clipped rather than passed on.
            application_name: clipped(application),
            application_version: 0,
            engine_name: "corvid",
            engine_version: 0,
            api_version: openxr::Version::new(1, 0, 0),
        };
        let instance = entry
            .create_instance(&info, &wanted, &[])
            .map_err(Unavailable::from)?;
        let system = instance
            .system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|_| Unavailable::NoHeadset)?;

        // Whether the room can be shown at all is a property of the blend modes
        // the runtime offers for this headset, and asking costs nothing.
        let blended = instance
            .enumerate_environment_blend_modes(system, STEREO)
            .is_ok_and(|modes| {
                modes.iter().any(|mode| {
                    *mode == openxr::EnvironmentBlendMode::ALPHA_BLEND
                        || *mode == openxr::EnvironmentBlendMode::ADDITIVE
                })
            });

        Ok(Self {
            _entry: entry,
            instance,
            system,
            state: State::Idle,
            session: None,
            predicted: openxr::Time::from_nanos(0),
            began: 0,
            passthrough: if blended {
                Passthrough::Off
            } else {
                Passthrough::Unavailable
            },
            head: Tracked::default(),
            views: Tracked::default(),
            hands: [Tracked::default(); 2],
        })
    }

    /// Starts a session on the device `wgpu` opened.
    ///
    /// The device must stay alive for as long as this `OpenXr` does, which is
    /// what a game holding both in one struct gives for nothing.
    ///
    /// # Errors
    ///
    /// [`Unavailable::DeviceMismatch`] when the device is not the Vulkan one
    /// the runtime named — or is not a Vulkan device at all — and
    /// [`Unavailable::Runtime`] when the runtime refused.
    pub fn open(&mut self, device: &wgpu::Device) -> Result<(), Unavailable> {
        if self.session.is_some() {
            return Ok(());
        }
        let handles = vulkan::handles(device)?;
        let wanted = vulkan::graphics_device(&self.instance, self.system, handles.instance)?;
        if wanted != handles.physical_device {
            return Err(Unavailable::DeviceMismatch);
        }

        let (session, waiter, stream) = vulkan::session(&self.instance, self.system, handles)?;
        let stage = session
            .create_reference_space(openxr::ReferenceSpaceType::STAGE, openxr::Posef::IDENTITY)
            .map_err(Unavailable::from)?;
        let local = session
            .create_reference_space(openxr::ReferenceSpaceType::LOCAL, openxr::Posef::IDENTITY)
            .map_err(Unavailable::from)?;
        let view = session
            .create_reference_space(openxr::ReferenceSpaceType::VIEW, openxr::Posef::IDENTITY)
            .map_err(Unavailable::from)?;
        let actions = Actions::new(&self.instance, &session)?;

        self.session = Some(Running {
            session,
            waiter,
            stream,
            stage,
            local,
            view,
            actions,
        });
        Ok(())
    }

    /// Whether a session has been started.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.session.is_some()
    }

    /// What the runtime last said the lifecycle was doing.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

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
    /// The poses [`poll`](Headset::poll) reads are located at that time rather
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
    fn since_start(&self, time: openxr::Time) -> Duration {
        Duration::from_nanos((time.as_nanos() - self.began).max(0).unsigned_abs())
    }

    /// Reads the events the runtime has queued and moves the lifecycle.
    fn drain(&mut self) {
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
    fn locate(&mut self) {
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
            .amplitude(effect.amplitude.to_f64() as f32)
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

impl Actions {
    /// The one action set, bound to the profiles a controller answers to.
    fn new(
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
        // inputs at all — so the poses and the haptic are bound there, and the
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
    fn hand(
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

/// The per-eye render target.
///
/// One array texture with two layers when the device offers multiview, and two
/// single-layer textures when it does not. CI's software adapter does not, so
/// the two-pass path is the one that is tested and the multiview path is the
/// one only a headset runs.
pub struct Swapchain {
    /// The runtime's swapchain. Owns the images the textures below wrap, and is
    /// dropped after them.
    inner: openxr::Swapchain<openxr::Vulkan>,
    /// One per swapchain image.
    textures: Vec<wgpu::Texture>,
    /// How big each eye's image is.
    extent: Extent,
    /// Whether the two eyes are two layers of one texture.
    multiview: bool,
}

impl Swapchain {
    /// The format asked for, and the only one this crate reads.
    ///
    /// `VK_FORMAT_R8G8B8A8_SRGB`, which every runtime offers and which `wgpu`
    /// calls `Rgba8UnormSrgb`.
    const FORMAT: u32 = 43;

    /// Creates the swapchain and wraps its images.
    fn open(
        session: &openxr::Session<openxr::Vulkan>,
        device: &wgpu::Device,
        extent: Extent,
    ) -> Result<Self, Unavailable> {
        let offered = session
            .enumerate_swapchain_formats()
            .map_err(Unavailable::from)?;
        if !offered.contains(&Self::FORMAT) {
            return Err(Unavailable::NoFormat);
        }
        let multiview = device.features().contains(wgpu::Features::MULTIVIEW);
        let layers = if multiview { 2 } else { 1 };

        let inner = session
            .create_swapchain(&openxr::SwapchainCreateInfo {
                create_flags: openxr::SwapchainCreateFlags::EMPTY,
                usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | openxr::SwapchainUsageFlags::SAMPLED,
                format: Self::FORMAT,
                sample_count: 1,
                width: extent.width,
                height: extent.height,
                face_count: 1,
                array_size: layers,
                mip_count: 1,
            })
            .map_err(Unavailable::from)?;

        let size = wgpu::Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: layers,
        };
        let images = inner.enumerate_images().map_err(Unavailable::from)?;
        let mut textures = Vec::with_capacity(images.len());
        for image in images {
            textures.push(vulkan::texture(
                device,
                image,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                size,
                "corvid_xr swapchain",
            )?);
        }

        Ok(Self {
            inner,
            textures,
            extent,
            multiview,
        })
    }

    /// Takes this frame's image, and says which it is.
    ///
    /// Blocks until the compositor has finished with it, so a frame drawn into
    /// it is a frame the compositor has not read.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Runtime`] when the runtime refused, which includes the
    /// timeout it answers when the compositor is not keeping up.
    pub fn acquire(&mut self) -> Result<u32, Unavailable> {
        let index = self.inner.acquire_image().map_err(Unavailable::from)?;
        self.inner
            .wait_image(openxr::Duration::from_nanos(PATIENCE))
            .map_err(Unavailable::from)?;
        Ok(index)
    }

    /// Hands this frame's image back to the compositor.
    ///
    /// # Errors
    ///
    /// [`Unavailable::Runtime`] when the runtime refused.
    pub fn release(&mut self) -> Result<(), Unavailable> {
        self.inner.release_image().map_err(Unavailable::from)
    }

    /// One of the swapchain's textures, by the index [`acquire`](Self::acquire)
    /// answered with.
    #[must_use]
    pub fn texture(&self, index: u32) -> Option<&wgpu::Texture> {
        self.textures.get(index as usize)
    }

    /// Whether the two eyes are two layers of one texture.
    #[must_use]
    pub const fn multiview(&self) -> bool {
        self.multiview
    }

    /// How big each eye's image is.
    #[must_use]
    pub const fn extent(&self) -> Extent {
        self.extent
    }

    /// How many images the runtime is cycling through.
    #[must_use]
    pub const fn images(&self) -> usize {
        self.textures.len()
    }
}

impl core::fmt::Debug for OpenXr {
    /// What a person watching a session wants to see, rather than the handles.
    ///
    /// Written out rather than derived because none of the runtime's types
    /// implement `Debug`, and a wall of opaque pointers would say less than
    /// this does anyway.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenXr")
            .field("state", &self.state)
            .field("open", &self.session.is_some())
            .field("passthrough", &self.passthrough)
            .field("head", &self.head)
            .finish_non_exhaustive()
    }
}

impl core::fmt::Debug for Swapchain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Swapchain")
            .field("extent", &self.extent)
            .field("multiview", &self.multiview)
            .field("images", &self.textures.len())
            .finish_non_exhaustive()
    }
}

/// How big an image is, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Across.
    pub width: u32,
    /// Down.
    pub height: u32,
}

/// An application name clipped to what the runtime's buffer holds.
fn clipped(name: &str) -> &str {
    const ROOM: usize = 127;
    if name.len() <= ROOM {
        return name;
    }
    let mut end = ROOM;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// The lifecycle state a runtime's session state means here.
const fn lifecycle(state: openxr::SessionState) -> State {
    match state {
        openxr::SessionState::READY => State::Ready,
        openxr::SessionState::SYNCHRONIZED | openxr::SessionState::VISIBLE => State::Visible,
        openxr::SessionState::FOCUSED => State::Focused,
        openxr::SessionState::STOPPING => State::Stopping,
        openxr::SessionState::LOSS_PENDING | openxr::SessionState::EXITING => State::Exiting,
        _ => State::Idle,
    }
}

/// How much a located pose is to be believed.
fn believed(flags: openxr::SpaceLocationFlags) -> Confidence {
    if flags.contains(
        openxr::SpaceLocationFlags::POSITION_TRACKED
            | openxr::SpaceLocationFlags::ORIENTATION_TRACKED,
    ) {
        Confidence::Tracked
    } else if flags.intersects(
        openxr::SpaceLocationFlags::POSITION_VALID | openxr::SpaceLocationFlags::ORIENTATION_VALID,
    ) {
        Confidence::Inferred
    } else {
        Confidence::Lost
    }
}

/// The same, for a view state.
fn seen(flags: openxr::ViewStateFlags) -> Confidence {
    if flags.contains(
        openxr::ViewStateFlags::POSITION_TRACKED | openxr::ViewStateFlags::ORIENTATION_TRACKED,
    ) {
        Confidence::Tracked
    } else if flags.intersects(
        openxr::ViewStateFlags::POSITION_VALID | openxr::ViewStateFlags::ORIENTATION_VALID,
    ) {
        Confidence::Inferred
    } else {
        Confidence::Lost
    }
}

/// An `OpenXR` pose in this workspace's axes.
///
/// `OpenXR` is **+X** right, **+Y** up, **−Z** forward; this workspace is **+X**
/// right, **+Y** forward, **+Z** up. The two differ by a quarter turn about
/// **X**, which is a proper rotation — so a position's components swap and one
/// negates, and a quaternion's vector part does the same while its scalar is
/// left alone.
fn pose(from: openxr::Posef) -> Pose {
    let position = GlobalFinePoint::new(
        I48F16::from_f64(f64::from(from.position.x)),
        I48F16::from_f64(f64::from(-from.position.z)),
        I48F16::from_f64(f64::from(from.position.y)),
    );
    let turn = |value: f32| I2F30::from_f64(f64::from(value));
    let rotation = Versor::from_xyzw(
        turn(from.orientation.x),
        turn(-from.orientation.z),
        turn(from.orientation.y),
        turn(from.orientation.w),
    )
    .map_or(FineRotation::IDENTITY, FineRotation::from_versor);
    Pose::new(position, rotation)
}

/// A runtime's zero-to-one analogue value as a [`Factor16`](corvid_fixed::Factor16).
fn factor(value: f32) -> corvid_fixed::Factor16 {
    corvid_fixed::Factor16::from_f64(f64::from(value.clamp(0.0, 1.0)))
}
