//! The device, the target, and the frame the game records into.

use std::time::Duration;

use crate::render::Target;

/// How long [`Renderer::read_back`] will wait for a device to hand a frame
/// over before calling it wedged.
///
/// A read-back is one texture copy of one frame, so on any adapter that is
/// working this is over in milliseconds and the number does not matter. What it
/// is for is the adapter that is *not* working: `wgpu`'s own default is to wait
/// forever, and this call is on the per-frame path of a captured run, so a
/// driver that stops answering would park the whole runtime with no error and
/// no output. `corvid_render`'s and `corvid_app`'s device tests each carry a
/// watchdog thread against exactly that wedge; a shipping game has no test
/// harness to catch it, so the deadline belongs here as well.
///
/// Long enough that a loaded software rasteriser under a debugger is nowhere
/// near it.
const PATIENCE: Duration = Duration::from_secs(30);

/// How many pixels wide and tall a target is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Extent {
    /// An extent from its two numbers.
    #[must_use]
    #[inline]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports and what nothing can be drawn into.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Width over height, or one for an extent with no height.
    ///
    /// What a projection wants, and the reason it is here rather than in the
    /// game: a zero-height target has no aspect ratio, and the answer that
    /// draws nothing is better than the infinity that spreads through a matrix.
    #[must_use]
    #[inline]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a viewport is at most 65535 pixels on any target this workspace builds for, and an f32 counts integers exactly to 16.7 million"
    )]
    pub fn aspect(self) -> f32 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// The same extent with neither dimension below one.
    ///
    /// A texture of zero width cannot be created, and a surface of zero width
    /// cannot be configured, so this is what a minimised window is stored as.
    #[must_use]
    #[inline]
    const fn at_least_one(self) -> Self {
        Self {
            width: if self.width == 0 { 1 } else { self.width },
            height: if self.height == 0 { 1 } else { self.height },
        }
    }
}

/// How eagerly a windowed renderer hands finished frames over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pacing {
    /// Wait for the display. Every frame is shown, none is torn, and the loop
    /// runs at the refresh rate.
    #[default]
    Display,
    /// Do not wait. The loop runs as fast as it can and the display shows
    /// whatever was finished when it looked, which tears and is what a latency
    /// measurement wants.
    Immediate,
}

/// Something went wrong setting up or drawing with a device.
///
/// Nothing here is a game's frame being wrong. What a game records is `wgpu`
/// calls, which report their own problems through `wgpu`'s device error
/// handling, so every case below is about a device, a window, or a buffer.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The window would not become a surface.
    Surface(wgpu::CreateSurfaceError),
    /// No adapter would serve. On a machine with no GPU at all this is what a
    /// missing software adapter looks like.
    NoAdapter(wgpu::RequestAdapterError),
    /// An adapter was found and would not open a device.
    NoDevice(wgpu::RequestDeviceError),
    /// The surface could not hand over a texture to draw into, and
    /// reconfiguring did not help.
    NoFrame(Unacquired),
    /// A frame could not be read back off the device.
    NotRead(String),
    /// [`Renderer::read_back`] was called on a renderer that draws into a
    /// window. A window's frame belongs to the compositor once it is
    /// presented, so there is nothing left here to read.
    NotOffscreen,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(why) => write!(f, "this window did not become a surface: {why}"),
            Self::NoAdapter(why) => write!(f, "no adapter would serve: {why}"),
            Self::NoDevice(why) => write!(f, "the adapter would not open a device: {why}"),
            Self::NoFrame(why) => write!(f, "the surface has no frame to draw into: {why:?}"),
            Self::NotRead(why) => write!(f, "the frame could not be read back: {why}"),
            Self::NotOffscreen => f.write_str(
                "this renderer draws into a window, and a presented frame belongs to the \
                 compositor rather than to us",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Surface(why) => Some(why),
            Self::NoAdapter(why) => Some(why),
            Self::NoDevice(why) => Some(why),
            Self::NoFrame(_) | Self::NotRead(_) | Self::NotOffscreen => None,
        }
    }
}

/// Why a surface would not hand over a texture to draw into.
///
/// The two a caller can do something about — a surface that has gone out of
/// date and one that has been lost — are handled inside
/// [`Renderer::frame`] by configuring again, so reaching this type means the
/// second attempt failed too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unacquired {
    /// The surface configuration no longer matches the window.
    Outdated,
    /// The surface itself has to be built again, which needs the window back.
    Lost,
    /// A validation error was raised inside the request.
    Validation,
}

/// One frame, read back off the device.
///
/// Four bytes per pixel, row by row from the top, in the order red, green,
/// blue, alpha. [`to_png`](Self::to_png) is what a capture writes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Image {
    /// How big it is.
    pub size: Extent,
    /// `size.width * size.height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Image {
    /// The same frame as the bytes of a PNG file.
    ///
    /// Lossless and eight bits a channel, so what comes back out of a decoder
    /// is what went in — which is what lets a capture be compared at all.
    ///
    /// # Errors
    ///
    /// [`Error::NotRead`], reused rather than given a variant of its own,
    /// because everything the encoder can refuse here is this image being
    /// malformed: a `pixels` that is not four bytes per pixel of `size`. A
    /// [`read_back`](Renderer::read_back) result never is.
    pub fn to_png(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, self.size.width, self.size.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|why| Error::NotRead(why.to_string()))?;
        writer
            .finish()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        Ok(bytes)
    }
}

/// Where a finished frame goes.
#[derive(Debug)]
enum Canvas {
    /// A window's surface.
    Window {
        /// The surface itself.
        surface: wgpu::Surface<'static>,
        /// How it is configured, kept so a resize is one field and a
        /// reconfigure.
        config: wgpu::SurfaceConfiguration,
    },
    /// A texture nobody sees until it is read back.
    Offscreen {
        /// The texture.
        texture: wgpu::Texture,
        /// A view of it, kept beside it rather than made per frame.
        ///
        /// A window hands back a different texture every frame and so has to
        /// make a view every frame; this texture is the same object until
        /// [`resize`](Renderer::resize) replaces it, so making one per frame
        /// was a backend image-view allocated and destroyed once a frame to
        /// describe something that had not changed. `wgpu::TextureView` is
        /// reference-counted, so handing out a clone is a refcount rather than
        /// a device call.
        view: wgpu::TextureView,
    },
}

/// The device, the target, and the acquire-record-submit-present step.
///
/// # What it does with a game's frame, and what it does not
///
/// It opens the device, acquires somewhere to draw, opens an encoder, hands
/// the four of them to [`frame`](Self::frame)'s closure, and submits. It owns
/// no pipeline, no shader, no material, no light, no pass and no scene graph —
/// there is nothing in this type that knows what a frame contains.
///
/// Nothing comes back to the game either. The only value `frame` produces is
/// whether a *device* stopped working, and a simulation cannot read it because
/// a simulation does not call it. That is the rule the whole client ring is
/// built on, stated as a signature.
#[derive(Debug)]
pub struct Renderer {
    /// The open device.
    device: wgpu::Device,
    /// Its queue.
    queue: wgpu::Queue,
    /// Where frames go.
    canvas: Canvas,
    /// What format the colour attachment is.
    colour: wgpu::TextureFormat,
    /// How big the target is.
    size: Extent,
}

impl Renderer {
    /// Opens a device and draws into a window.
    ///
    /// `target` is anything `wgpu` will make a surface out of, which in this
    /// workspace is `corvid_window`'s `Surface`. It is taken by value and kept
    /// for the renderer's life, because a surface borrows its window and a
    /// window that outlives its surface is the one thing this cannot be asked
    /// to check.
    ///
    /// # Errors
    ///
    /// [`Error::Surface`] if the window will not become one, [`Error::NoAdapter`]
    /// if nothing on the machine will serve it, and [`Error::NoDevice`] if the
    /// adapter that would refuses to open.
    pub fn for_window(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        size: Extent,
        pacing: Pacing,
    ) -> Result<Self, Error> {
        let instance = instance();
        let surface = instance.create_surface(target).map_err(Error::Surface)?;
        let (adapter, device, queue) = open(&instance, Some(&surface))?;

        let size = size.at_least_one();
        // The device's own answer, then three fields overridden. Building the
        // whole configuration by hand is how a renderer stops working on the
        // next `wgpu` that adds a field to it.
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .ok_or(Error::NoFrame(Unacquired::Validation))?;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        config.format = choose_format(&surface.get_capabilities(&adapter).formats);
        config.present_mode = match pacing {
            Pacing::Display => wgpu::PresentMode::AutoVsync,
            Pacing::Immediate => wgpu::PresentMode::AutoNoVsync,
        };
        let colour = config.format;
        surface.configure(&device, &config);

        Ok(Self {
            device,
            queue,
            canvas: Canvas::Window { surface, config },
            colour,
            size,
        })
    }

    /// Opens a device and draws into a texture nobody sees.
    ///
    /// The headless path with a real adapter on it: the same device, the same
    /// `Render` implementation, the same recording. What it does not have is a
    /// window, which is what lets it run on a build machine and what makes
    /// [`read_back`](Self::read_back) possible.
    ///
    /// # Errors
    ///
    /// [`Error::NoAdapter`] if nothing on the machine will serve — including a
    /// software rasteriser, which is what a build machine usually has — and
    /// [`Error::NoDevice`] if the adapter that would refuses to open.
    pub fn offscreen(size: Extent) -> Result<Self, Error> {
        let instance = instance();
        let (_, device, queue) = open(&instance, None)?;

        let size = size.at_least_one();
        // Not an `Srgb` format: what a capture reads back should be the bytes
        // the game's shader wrote, and an `Srgb` target would encode them on
        // the way in.
        let colour = wgpu::TextureFormat::Rgba8Unorm;
        let texture = colour_texture(&device, size, colour);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            device,
            queue,
            canvas: Canvas::Offscreen { texture, view },
            colour,
            size,
        })
    }

    /// The open device, for building whatever a game builds once.
    #[must_use]
    #[inline]
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Its queue.
    #[must_use]
    #[inline]
    pub const fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// What format the colour attachment is, which a pipeline is built for.
    #[must_use]
    #[inline]
    pub const fn format(&self) -> wgpu::TextureFormat {
        self.colour
    }

    /// How big the target is.
    #[must_use]
    #[inline]
    pub const fn size(&self) -> Extent {
        self.size
    }

    /// Tells the renderer the target changed size.
    ///
    /// Cheap and idempotent: a resize to the size it already is does nothing,
    /// which matters because a window reports its size far more often than it
    /// changes. What a game keeps at the target's size — a depth texture, most
    /// often — is the game's to rebuild, and [`Target::size`] is where it
    /// notices.
    pub fn resize(&mut self, size: Extent) {
        let size = size.at_least_one();
        if size == self.size {
            return;
        }
        self.size = size;
        match &mut self.canvas {
            Canvas::Window { surface, config } => {
                config.width = size.width;
                config.height = size.height;
                surface.configure(&self.device, config);
            }
            Canvas::Offscreen { texture, view } => {
                *texture = colour_texture(&self.device, size, self.colour);
                *view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            }
        }
    }

    /// Acquires a target, opens an encoder, lets `record` fill it, submits and
    /// presents.
    ///
    /// `record` is not called at all on a frame there is nowhere to draw —
    /// a minimised window, a fully occluded one, a compositor that timed out —
    /// because there is no texture to hand it. That is what the platform asked
    /// for and is not something a game can act on.
    ///
    /// Answers whether `record` ran, which is not the same question as whether
    /// this succeeded. A caller that counts displayed frames, or writes one row
    /// of a capture per displayed frame, wants the first: a minimised window
    /// answers `Ok(false)` for as long as it stays minimised, and counting
    /// those is a frame count that keeps climbing while nothing is on screen.
    ///
    /// # Errors
    ///
    /// [`Error::NoFrame`] when the surface will not hand over a texture even
    /// after being reconfigured, which is what a device removed mid-session
    /// looks like.
    pub fn frame(&mut self, record: impl FnOnce(Target<'_>)) -> Result<bool, Error> {
        let Some((view, presenting)) = self.acquire()? else {
            return Ok(false);
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("corvid_render.frame"),
            });
        record(Target {
            device: &self.device,
            queue: &self.queue,
            encoder: &mut encoder,
            view: &view,
            format: self.colour,
            size: self.size,
        });
        self.queue.submit(std::iter::once(encoder.finish()));
        if let Some(frame) = presenting {
            self.queue.present(frame);
        }
        Ok(true)
    }

    /// The texture this frame is drawn into, and the surface frame to present
    /// afterwards if there is one.
    ///
    /// [`None`] means there is nowhere to draw this frame and that is fine: a
    /// minimised window is occluded and a compositor under load times out, and
    /// neither is something a game can act on.
    fn acquire(&self) -> Result<Option<(wgpu::TextureView, Option<wgpu::SurfaceTexture>)>, Error> {
        use wgpu::CurrentSurfaceTexture as Acquired;

        // A `match` rather than a `let ... else` inside a `let ... else`,
        // because the inner `else` of the nested form is a branch that no value
        // of a two-variant enum can reach — and one a third variant would fall
        // into silently, answering "nowhere to draw this frame" and turning
        // every frame into `Ok(false)` where a compile error was wanted.
        let (surface, config) = match &self.canvas {
            Canvas::Offscreen { view, .. } => return Ok(Some((view.clone(), None))),
            Canvas::Window { surface, config } => (surface, config),
        };

        let frame = match surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Timeout | Acquired::Occluded => return Ok(None),
            _ => {
                // The compositor changed something under us — a resize that
                // arrived between two frames, a monitor reconfigured.
                // Configuring again is the documented recovery, and a second
                // failure is a real one.
                surface.configure(&self.device, config);
                match surface.get_current_texture() {
                    Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
                    Acquired::Timeout | Acquired::Occluded => return Ok(None),
                    // The *second* answer, which is the one that is not
                    // recoverable. Reporting the first said "out of date" for a
                    // surface that had since been lost, and those ask a caller
                    // for opposite things: one wants configuring again, which
                    // has just been tried, and the other wants the surface
                    // rebuilt from the window.
                    again => return Err(Error::NoFrame(unacquired(&again))),
                }
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        Ok(Some((view, Some(frame))))
    }

    /// Reads the last drawn frame back off the device.
    ///
    /// This is the whole capture seam. There is no draw list to serialize any
    /// more, so what a headless run can be compared on is the pixels a real
    /// adapter produced — and the crate documentation is exact about how much
    /// weaker a golden that makes.
    ///
    /// # Errors
    ///
    /// [`Error::NotOffscreen`] on a renderer that draws into a window, and
    /// [`Error::NotRead`] if the device will not hand the bytes over.
    pub fn read_back(&self) -> Result<Image, Error> {
        let Canvas::Offscreen { texture, .. } = &self.canvas else {
            return Err(Error::NotOffscreen);
        };

        // A copy out of a texture writes rows padded to 256 bytes, so the
        // buffer is wider than the image and the padding is dropped below.
        let unpadded = self.size.width * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("corvid_render.readback"),
            size: u64::from(padded) * u64::from(self.size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("corvid_render.readback"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.size.height),
                },
            },
            wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            // The receiver is on the stack below and outlives this, so the
            // only way the send fails is a device that vanished, in which case
            // the `recv` below reports the disconnect instead.
            drop(sender.send(result));
        });
        drop(self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(PATIENCE),
        }));
        match receiver.recv_timeout(PATIENCE) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => return Err(Error::NotRead(why.to_string())),
            Err(why) => return Err(Error::NotRead(format!("{why} after {PATIENCE:?}"))),
        }

        let mapped = slice
            .get_mapped_range()
            .map_err(|why| Error::NotRead(why.to_string()))?;
        let mut pixels = Vec::with_capacity((unpadded * self.size.height) as usize);
        for row in 0..self.size.height {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();

        Ok(Image {
            size: self.size,
            pixels,
        })
    }
}

/// Whether the adapter this machine would draw with is a software rasteriser.
///
/// This is what makes the exact-match arm of a PNG comparison expressible.
/// Rasterisation differs between drivers, so a captured frame is compared with
/// a tolerance in general — but two runs on the *same* software rasteriser
/// produce the same bytes, so a build machine with `lavapipe` can demand
/// equality and catch a change a tolerance would absorb.
///
/// It requests an adapter with the options [`Renderer::offscreen`] uses and
/// drops it again, so it names the adapter a run on this machine would open
/// rather than whichever one happens to be first. A machine with no adapter at
/// all answers `false`, because there is nothing there to pin an exact
/// comparison to.
///
/// With [`SOFTWARE`] set it names the fallback adapter, which is the one every
/// path in this module opens under that variable — so the answer stays "the one
/// a run on this machine would open" rather than becoming a claim about a
/// different device.
#[must_use]
pub fn adapter_is_software() -> bool {
    let instance = instance();
    request(&instance, None)
        .is_ok_and(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu)
}

/// The variable that makes every adapter request in this module ask for the
/// fallback.
///
/// A frame golden's exact-match arm is pinned to a software rasteriser, and a
/// developer machine usually has a GPU that `wgpu` will hand over first —
/// `WGPU_ADAPTER_NAME` does not help, because that is read by
/// `wgpu::util::initialize_adapter_from_env` and nothing here calls it. Setting
/// this to anything non-empty passes `force_fallback_adapter` instead, which is
/// `lavapipe` on a machine with Mesa and WARP on Windows.
///
/// **Two software rasterisers are not one adapter.** Blessing a golden under
/// this variable pins it to whichever fallback *this* machine has, and a build
/// machine with a different one is in the same position it would be with a GPU.
/// What the variable buys is that the pinning is a decision somebody made
/// rather than an accident of which adapter was first.
pub const SOFTWARE: &str = "CORVID_SOFTWARE_ADAPTER";

/// Whether [`SOFTWARE`] asks for the fallback adapter.
fn force_fallback() -> bool {
    std::env::var_os(SOFTWARE).is_some_and(|value| !value.is_empty())
}

/// The instance every path here opens.
fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// Asks for the adapter a run on this machine would use.
fn request(
    instance: &wgpu::Instance,
    compatible: Option<&wgpu::Surface<'static>>,
) -> Result<wgpu::Adapter, Error> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: force_fallback(),
        compatible_surface: compatible,
        apply_limit_buckets: false,
    }))
    .map_err(Error::NoAdapter)
}

/// Opens an adapter and a device.
fn open(
    instance: &wgpu::Instance,
    compatible: Option<&wgpu::Surface<'static>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), Error> {
    let adapter = request(instance, compatible)?;
    let info = adapter.get_info();
    tracing::info!(
        name: "corvid_render.adapter",
        adapter = %info.name,
        backend = %info.backend,
        device_type = ?info.device_type,
        "opened an adapter",
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("corvid_render"),
        // Nothing here needs anything past what every target has, which is what
        // keeps this from being the reason a machine cannot run the game. A
        // game that needs more opens its own device; this is the one a runtime
        // opens for it.
        required_features: wgpu::Features::empty(),
        // The downlevel baseline for everything except how big a texture may
        // be, which is raised to whatever this adapter actually offers.
        //
        // The baseline's own answer is 2048, and a swapchain is a texture: a
        // maximised window on any display wider than that is not a recoverable
        // `Error` but a `ConfigureSurfaceError::TooLarge` on an infallible
        // `configure`, which `wgpu`'s default handler turns into a panic. The
        // same 2048 caps `Renderer::offscreen`, whose signature promises a
        // `Result` it never gets to return. `using_resolution` is `wgpu`'s own
        // constructor for this: the portable floor for every limit that
        // describes what a shader may do, and the hardware's ceiling for the
        // one that only describes how many pixels there are.
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(Error::NoDevice)?;
    Ok((adapter, device, queue))
}

/// Which of a surface's formats to configure it with.
///
/// A non-`Srgb` format is preferred, so that what a game's shader writes
/// reaches the display as the numbers it wrote: this crate has no colour
/// management, and an `Srgb` target would encode values that were never
/// decoded.
fn choose_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .or_else(|| formats.first().copied())
        .unwrap_or(wgpu::TextureFormat::Bgra8Unorm)
}

/// Which [`Unacquired`] a `wgpu` answer means.
const fn unacquired(answer: &wgpu::CurrentSurfaceTexture) -> Unacquired {
    match answer {
        wgpu::CurrentSurfaceTexture::Lost => Unacquired::Lost,
        wgpu::CurrentSurfaceTexture::Outdated => Unacquired::Outdated,
        _ => Unacquired::Validation,
    }
}

/// The colour texture an offscreen renderer draws into.
fn colour_texture(
    device: &wgpu::Device,
    size: Extent,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("corvid_render.offscreen"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
