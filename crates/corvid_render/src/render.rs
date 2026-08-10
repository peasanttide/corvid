//! The drawing half of a game's client-local code, and what it is handed.

use corvid_behavior::{Data, Extract, Loading, State};
use corvid_camera::Camera;
use corvid_fixed::Factor16;
use corvid_time::Time;

use crate::icon::Icon;
use crate::renderer::Extent;

/// Everything a game needs to record one frame, and nothing else.
///
/// This is the whole of what `corvid_render` decides about a frame: that there
/// is a texture to draw into, a format it is in, a size it is, and an encoder
/// already open on a device. What passes exist, what they clear to, what is
/// depth-tested, what a material is and where the light comes from are the
/// game's, because every abstraction over a GPU is a bet about which games
/// exist and this crate is not in that business.
///
/// The four references are real `wgpu`. Begin as many render passes on
/// [`encoder`](Self::encoder) as the frame wants, write buffers through
/// [`queue`](Self::queue), build a texture with [`device`](Self::device) —
/// there is no wrapper in the way of any of it.
///
/// # Why the encoder is already open
///
/// Submission is paced. A windowed renderer acquires a surface texture,
/// records, submits and presents in one step, and a frame that submitted
/// halfway through would be presented halfway through. So the encoder is
/// opened by [`Renderer::frame`](crate::Renderer::frame), handed over here,
/// and finished and submitted after [`Render::draw`] returns. A game that
/// genuinely wants its own submission — a compute prepass whose results this
/// frame reads back — has [`device`](Self::device) and can make a second
/// encoder of its own.
#[derive(Debug)]
pub struct Target<'a> {
    /// The open device, for anything that has to be created mid-frame: a depth
    /// texture that follows the window's size, a bind group over a buffer that
    /// has just grown.
    pub device: &'a wgpu::Device,
    /// The queue, for `write_buffer` and `write_texture`.
    pub queue: &'a wgpu::Queue,
    /// The encoder this frame is recorded into. Already open, submitted after
    /// [`Render::draw`] returns.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The colour attachment to draw into: a window's acquired surface
    /// texture, or the offscreen texture a capture is read back from.
    pub view: &'a wgpu::TextureView,
    /// What format [`view`](Self::view) is, which is what a pipeline's colour
    /// target has to be built for.
    ///
    /// It is chosen by the surface rather than by this crate and it is not the
    /// same on every machine, which is why it is handed to
    /// [`Render::new`](Render::new) as well: a pipeline built for the
    /// wrong format is a validation error on the first frame.
    pub format: wgpu::TextureFormat,
    /// How big [`view`](Self::view) is, in pixels.
    ///
    /// This is where a resize is noticed. Nothing else tells a renderer how big
    /// the window is — the window publishes its size and the renderer reads it
    /// here — so a renderer keeping a depth texture compares this against the
    /// size it last built one at.
    pub size: Extent,
}

/// The device a renderer builds its pipelines against.
///
/// [`Copy`]: it is handed to [`Render::new`] and read there, nowhere held
/// past the call.
#[derive(Clone, Copy, Debug)]
pub struct Opened<'a> {
    /// The device to create resources on.
    pub device: &'a wgpu::Device,
    /// The queue to submit on.
    pub queue: &'a wgpu::Queue,
    /// The surface's format, which is not the same on every machine.
    pub format: wgpu::TextureFormat,
}

/// What a renderer is handed for one frame.
///
/// One struct rather than five arguments, so that a new thing to hand over is
/// a field here and not a signature change in every implementation.
///
/// Not generic over the state, because nothing in it is: a renderer reads the
/// state through [`Extract`](../corvid_behavior/trait.Extract.html) into its
/// own types, and by the time a frame is drawn what is left is a camera, a
/// target and two numbers.
#[derive(Debug)]
pub struct Drawing<'a> {
    /// Where the frame goes.
    pub target: Target<'a>,
    /// Whatever the controller's `look` answered.
    pub camera: &'a Camera,
    /// How far along this machine's bytes are, while a level is being read.
    pub loading: Option<Loading<'a>>,
    /// Where the session is.
    pub time: Time,
    /// The weight between the two extracted states: [`ZERO`](Factor16::ZERO)
    /// is the older.
    pub alpha: Factor16,
}

/// What a game draws with, and how.
///
/// One of the four types an [`App`](../corvid_app/struct.App.html) is made of,
/// and the one that is an *eye*. It is implemented for the renderer's **own**
/// type, which its own crate owns — so an art crate can write one against a
/// simulation crate's state with no marker type between them, and that is the
/// reason the marker type could be deleted at all.
///
/// # The renderer *is* the graphics
///
/// There is no `Graphics` associated type and no `Setup` trait. Splitting them
/// — a `Render` declaring what its pipelines are and a second trait saying how
/// to build them — is what a trait implemented for a *marker* would need, since
/// a marker cannot itself hold a `wgpu::RenderPipeline`. `Self` holds them, so
/// both collapse into [`new`](Self::new).
///
/// # Interpolation is this trait's, and it happens on the GPU
///
/// [`extract`](Extract::extract) runs at most once per displayed frame and
/// pushes the pair; [`draw`](Self::draw) is handed the weight between them and
/// hands it to a shader. Nothing on that path is hashed, sent or compared
/// against a golden, which is why an `f32` lerp is allowed here and nowhere
/// below.
///
/// The cost of that arrangement is stated on [`Extract`]: after a rollback the
/// pair can span more than one tick, so the GPU lerps across a gap.
pub trait Render<S: State>: Extract<S> {
    /// What a player has set: resolution scale, shadow quality, gamma.
    ///
    /// The renderer's half of a game's settings, and never
    /// [`Rules`](corvid_behavior::State::Rules): it changes what one machine
    /// draws and not what any machine computes.
    type Config: Data;

    /// Whether this renderer wants an adapter.
    ///
    /// `false` means the runtime opens no device, acquires no surface and never
    /// calls [`draw`](Self::draw) — which is what makes a dedicated server and
    /// a determinism check cost nothing.
    ///
    /// It does **not** mean `wgpu` leaves the build. That line was given up
    /// deliberately: this crate is a hard dependency of the runtime, so a
    /// headless build still links a graphics stack it never opens. What was
    /// bought by the arrangement it replaced — a Cargo feature deciding whether
    /// this crate compiled at all — was true only of a workspace where nothing
    /// whatsoever enabled the feature, and Cargo unifies features across a
    /// workspace.
    const REAL: bool = true;

    /// Build the pipelines.
    ///
    /// Called once, when the device is opened, and never on a run with
    /// [`REAL`](Self::REAL) unset. `opened.format` is the surface's own and is
    /// not the same on every machine; a pipeline built for the wrong one is a
    /// validation error on the first frame.
    fn new(opened: Opened<'_>, config: Self::Config) -> Self;

    /// The player changed a setting while the game was running.
    ///
    /// It is handed no device, so a renderer that must rebuild has to hold the
    /// handles it needs. That is deliberate: a `configure` that could open
    /// resources is a `new` that runs at an arbitrary moment.
    fn configure(&mut self, config: Self::Config);

    /// Record one frame.
    ///
    /// `drawing.alpha` is the weight between the two states
    /// [`extract`](Extract::extract) pushed: [`ZERO`](Factor16::ZERO) is the
    /// older and [`ONE`](Factor16::ONE) the newer. It goes into a uniform and
    /// the shader lerps.
    ///
    /// `drawing.camera` is whatever the controller's `look` answered, so the
    /// eye and the ears are in the same place without either being told
    /// twice.
    ///
    /// `drawing.loading` is present only while a level is being read, and
    /// carries how far along **this machine's** bytes are. Whether the game
    /// *is* loading is in the state, because every peer agrees about that;
    /// how far along one disk has got is nobody else's business, which is why
    /// it is here instead.
    fn draw(&mut self, drawing: Drawing<'_>);

    /// The picture a platform puts in the title bar, the dock and the task
    /// switcher, or [`None`] to leave whatever the platform would have used.
    ///
    /// Asked once, when a window is opened, and never on a run that has none —
    /// which is why it is an associated function with no `self`. It is here
    /// rather than on the controller because an icon is a picture, and this is
    /// the half of a game that deals in those.
    #[must_use]
    fn icon() -> Option<Icon>
    where
        Self: Sized,
    {
        None
    }
}

/// A game with nothing to draw.
///
/// The default for an [`App`](../corvid_app/struct.App.html)'s renderer, and
/// the whole of what a dedicated server owes: no device is opened, no surface
/// is acquired, and [`draw`](Render::draw) is never called.
impl<S: State> Render<S> for () {
    type Config = ();

    const REAL: bool = false;

    fn new(_opened: Opened<'_>, (): ()) -> Self {}

    fn configure(&mut self, (): ()) {}

    fn draw(&mut self, _drawing: Drawing<'_>) {}
}
