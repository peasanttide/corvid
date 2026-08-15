//! A real headset, through `OpenXR`. Behind the `openxr` feature.
//!
//! Everything a game touches is the same vocabulary the stand-in speaks: this
//! module implements [`Headset`](crate::Headset) a second time, and nothing above it knows
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
//! passthrough is on offer, and the swapchain -- including handing back the
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
//! offer, a frame the compositor drops -- the stand-in stops these paths from
//! rotting, and a headset in somebody's hands is the only thing that certifies
//! them.

mod vulkan;

use crate::{Hand, Passthrough, Pose, State, Tracked, Views};

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

mod actions;
mod convert;
mod frame;
mod headset;
mod swapchain;
mod unavailable;

pub use swapchain::{Extent, Swapchain};
pub use unavailable::Unavailable;

use convert::clipped;

/// A real headset: the runtime this process found, the device it was handed,
/// and the session the two of them are in.
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
    /// the runtime named -- or is not a Vulkan device at all -- and
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
