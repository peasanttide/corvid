//! The device, the target, and the frame the game records into.
//!
//! The seam between this file and the four beside it is *when* each part is
//! reached. [`Renderer`] is the per-frame path -- acquire, record, submit,
//! present -- and `adapter.rs` is the once-per-run path that opens the device
//! it holds. `extent.rs`, `error.rs` and `image.rs` are the values crossing
//! either boundary, which nothing in either path has an opinion about.

use std::time::Duration;

use crate::render::Target;

mod adapter;
mod error;
mod extent;
mod image;
mod readback;

pub use adapter::{SOFTWARE, adapter_is_software};
pub use error::{Error, Unacquired};
pub use extent::{Extent, Pacing};
pub use image::Image;

use adapter::{choose_format, colour_texture, instance, open, unacquired};

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

/// Where a finished frame goes.
#[derive(Debug)]
pub(super) enum Canvas {
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
/// no pipeline, no shader, no material, no light, no pass and no scene graph --
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
    /// [`Error::NoAdapter`] if nothing on the machine will serve -- including a
    /// software rasteriser, which is what a build machine usually has -- and
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
    /// changes. What a game keeps at the target's size -- a depth texture, most
    /// often -- is the game's to rebuild, and [`Target::size`] is where it
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
    /// `record` is handed the target and the open encoder as two arguments
    /// rather than one struct, because the encoder is the only thing it
    /// borrows mutably and a [`Target`] that held it could not be [`Copy`].
    ///
    /// `record` is not called at all on a frame there is nowhere to draw --
    /// a minimised window, a fully occluded one, a compositor that timed out --
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
    pub fn frame(
        &mut self,
        record: impl FnOnce(Target<'_>, &mut wgpu::CommandEncoder),
    ) -> Result<bool, Error> {
        let Some((view, presenting)) = self.acquire()? else {
            return Ok(false);
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("corvid_render.frame"),
            });
        record(
            Target {
                device: &self.device,
                queue: &self.queue,
                view: &view,
                format: self.colour,
                size: self.size,
            },
            &mut encoder,
        );
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
        // of a two-variant enum can reach -- and one a third variant would fall
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
                // The compositor changed something under us -- a resize that
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
}
