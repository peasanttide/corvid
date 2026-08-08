//! A deterministic multiplayer cross platform game framework.
//!
//! This is the facade: one name in a game's `Cargo.toml`, and everything the
//! game touches re-exported from behind it. A game that depends on `corvid`
//! names no other Corvid crate in its manifest and no other Corvid crate in its
//! source.
//!
//! ```toml
//! [dependencies]
//! corvid = "0.1"
//! ```
//!
//! ```rust
//! use corvid::{App, Level, Malformed, Source, State};
//! use serde::{Deserialize, Serialize};
//!
//! /// A level with nothing in it, read from nothing.
//! #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
//! struct Nowhere;
//!
//! impl Level for Nowhere {
//!     type Reference = String;
//!     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> {
//!         Ok(Self)
//!     }
//! }
//!
//! /// A game's own state, and the game. `Hash` is what a mark is taken
//! /// through; `Default` is what a fresh session opens on.
//! #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
//! struct Still;
//!
//! impl State for Still {
//!     const NAME: &'static str = "nothing";
//!     type Level = Nowhere;
//!     type Rules = ();
//!     type Action = ();
//! }
//!
//! // Four types, a name, and nothing else: `tick` and `load_level` both have
//! // defaults, so a game that does nothing writes no function at all.
//! //
//! // There is no controller, no renderer and no ear here either. `App`
//! // defaults all three to `()`, which opens no window, no adapter and no
//! // sound card — so this is a dedicated server, and the shortest way of
//! // drawing nothing is to say nothing.
//! let app = App::<Still>::new().headless().for_ticks(10);
//! ```
//!
//! # The four contracts, and which one a game starts with
//!
//! A Corvid game is four types, each carrying one of these:
//!
//! | | What it says |
//! |---|---|
//! | [`State`] | What a tick is: the state, the level, the rules, the actions, and the one function that advances them |
//! | [`Controller`] | Everything client-local that is not a device: the input declaration, `action`, `look` and what a pad should feel |
//! | [`Render`] | What a frame is drawn with: the game's `Graphics`, and the `draw` that uses them |
//! | [`Auralizer`] | What a frame sounds like: the `hear` that fills an [`AudioFrame`] from a state |
//!
//! A headless run needs only the first. The other three are what a player in
//! front of the run reads, hears and sees, and [`App`] defaults every one of
//! them to `()` — which is why the example above is a whole dedicated server
//! that never names a controller, a renderer or an ear.
//!
//! # What is behind it, and what is not
//!
//! The simulation ring, the whole of the client ring — the four contracts, the
//! renderer, the audio frame, the input snapshot, [`color`], [`shape`] and
//! [`camera`] — and the runtime. All of that is reachable here
//! unconditionally.
//!
//! That is a wider "unconditionally" than it used to be. A camera is
//! fixed-point state, a raycast is integer arithmetic and a colour is four
//! bytes, so none of those ever needed a feature; what is new is that
//! [`Render`], [`Target`], `wgpu` and the mesh crates do not either. `Present`
//! is built on `Render`, so a build of this crate compiles a graphics stack
//! whatever it was asked for, and a feature gating one would gate nothing. What
//! it does not do is *open* a device: a headless run never asks an adapter for
//! anything, which is the property that was worth having and the one that
//! survives.
//!
//! | Feature | Effect |
//! |---|---|
//! | `window` | Forwards `corvid_app/window`: a window, a keyboard, a pad and a sound card — everything that is only ever true of a run with a player in front of it. One name rather than three, because the three described the same machine |
//! | `dev` | Forwards `corvid_app/dev`: when a session diverges, `corvid_lockstep::bisect` runs and the report says which field moved first rather than only which tick. Adds no API of its own and changes nothing a build computes |
//!
//! # One name, and it is this one
//!
//! **`corvid` is the only Corvid crate a game names.** Every other crate in the
//! workspace names its own dependencies directly and re-exports none of them,
//! so this is the one place the whole surface is gathered — rather than a chain
//! of crates each forwarding its neighbour, where the same type could be
//! reached by four paths and a game had no way to tell which was intended.

// The runtime. `App`, `Opening`, the entry points and the argument parsing.
pub use corvid_app::*;

// The deterministic contract, and the digest a mark is.
pub use corvid_behavior::{
    self as behavior, Command, Data, Discard, ExitCode, Extract, Extracting, Level, Loading,
    Player, PlayerId, Presence, ProfileId, Scope, State, Time,
};
// The filesystem a level is read through, named here rather than forwarded by
// `corvid_behavior`. `Source` is in `Level::load`'s signature and `Malformed`
// is its error, so a game implementing the trait needs both — and this is the
// one crate in the workspace whose job is to save it naming two.
pub use corvid_files::{self as files, Malformed, Missing, Source};
pub use corvid_hash::{self as hash, Digest, Hasher, digest};

// The client-local half: what a player reads, hears and sees.
pub use corvid_control::{self as control, Controller};
pub use corvid_sound::{self as sound, AudioFrame, Auralizer, Cue, CueId, SoundId};
// Unconditional. Every `Controller` declares its input sets and is
// handed an `Input`, whether or not there is a device to fill one — a headless
// run passes an empty snapshot rather than skipping the call. What `window`
// adds is `platform`: the binding table and the device accumulator, which are
// the parts that only mean something with a keyboard in front of them.
#[cfg(feature = "window")]
pub use corvid_input::platform;
pub use corvid_input::{
    self as input, Analog, AnalogId, Axis, Button, Cursor, Digital, DigitalId, Input, Key,
    MouseButton, PadButton, PoseId, SetDescriptor, SetId, Viewport, action_sets,
};

// The session, the clock and the watch channel.
pub use corvid_replay::{
    self as replay, Opening, Opens, Profile, Schema, Seed, Session, Snapshots,
};
pub use corvid_signal::{self as signal, Emitter, Watch, channel};
pub use corvid_time::{self as time, Clock, Duration, Elapsed, Tick, TickSpan};

// The maths stack, from the bits up. A game writing a position, an angle or a
// colour names one crate for all of it.
pub use corvid_bits as bits;
pub use corvid_fixed::{
    self as fixed, Angle16, Angle32, Factor16, Factor32, I16F16, I24F8, I48F16, Pitch32, Signed32,
};
pub use corvid_float as float;
pub use corvid_glm::{self as glm, Mat4, Vec2, Vec3, Vec4};
pub use corvid_rotation::{self as rotation, Basis, FineRotation, Rotation, Versor};
pub use corvid_transform::{self as transform, GlobalFineTransform, Transform};
pub use corvid_vector::{
    self as vector, Direction, FinePoint, GlobalFinePoint, GlobalPoint, OctDirection,
};

// Geometry, cameras and colour: the client ring that is behind no device.
pub use corvid_camera::{self as camera, Camera, Eye, FirstPerson, Orbit};
pub use corvid_color::{self as color, LinearRgba, Rgba8};
pub use corvid_shape::{self as shape, Aabb, Cast, Frustum, Hit, Plane, Ray, Sphere, Triangle};
pub use corvid_ui as ui;

// Geometry and the device half of it.
#[cfg(feature = "window")]
pub use corvid_input::platform::Bindings;
pub use corvid_mesh::{self as mesh, Mesh, Vertex};
#[cfg(feature = "render")]
pub use corvid_mesh_render::{self as mesh_render, Uploaded};
#[cfg(feature = "render")]
pub use corvid_render::{self as render, Extent, Icon, Render, Target, wgpu};
#[cfg(feature = "render")]
pub use corvid_ui_render as ui_render;
#[cfg(feature = "window")]
pub use corvid_window::{self as window, Size};

// The console and the tunables, and the encoding a save is written in.
pub use corvid_dev as dev_console;
pub use corvid_wire as wire;
