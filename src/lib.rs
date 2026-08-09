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
//! use std::sync::Arc;
//!
//! use corvid::{
//!     App, Level, Malformed, Opening, Opens, Profile, ProfileId, Schema, Seed, Source, State,
//!     Tick, TickSpan, Ticks,
//! };
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
//! /// A game's own state. `Hash` is what a mark is taken through; `Default` is
//! /// what a fresh session opens on.
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
//! /// Where a fresh run starts: one seat, from tick zero, on the only level.
//! /// The one thing a runtime cannot invent for a game.
//! impl Opens for Still {
//!     fn opening() -> Opening<Self> {
//!         Opening {
//!             level: "nowhere".to_owned(),
//!             content: Arc::new(Nowhere),
//!             rules: Arc::new(()),
//!             roster: vec![Profile { account: ProfileId(1), joined: Tick::ZERO, left: None }],
//!             seed: Seed(0),
//!             first: Tick::ZERO,
//!             origin: None,
//!             schema: Schema::new("nothing").digest(),
//!         }
//!     }
//! }
//!
//! corvid::game! {
//!     /// The game, which is where its five types are declared and the only
//!     /// thing a run is given.
//!     ///
//!     /// Four of the five go unnamed and default to `()`: no controller, no
//!     /// bot, no renderer and no ear, which opens no window, no adapter and no
//!     /// sound card. So this is a whole dedicated server, and the shortest way
//!     /// of drawing nothing is to say nothing.
//!     struct Nothing;
//!     const PERIOD: TickSpan = TickSpan::CRADLE;
//!     type State = Still;
//! }
//!
//! let app = App::<Nothing>::new()
//!     .opening(Still::opening())
//!     .headless()
//!     .for_ticks(Ticks(10));
//! ```
//!
//! # One macro, and it is the whole binary
//!
//! [`app!`] is [`game!`] and the `main` that plays it, which is every line of a
//! Corvid game that is not the game. Both reach this facade through the same
//! glob the types above do, so a game that names `corvid` names nothing else:
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use corvid::{Level, Malformed, Opening, Profile, ProfileId, Schema, Seed, Source, State,
//! #     Tick, TickSpan};
//! # use serde::{Deserialize, Serialize};
//! # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
//! # struct Nowhere;
//! # impl Level for Nowhere {
//! #     type Reference = String;
//! #     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
//! # }
//! # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
//! # struct Still;
//! # impl State for Still {
//! #     const NAME: &'static str = "nothing";
//! #     type Level = Nowhere; type Rules = (); type Action = ();
//! # }
//! # impl corvid::Opens for Still {
//! #     fn opening() -> Opening<Self> {
//! #         Opening {
//! #             level: "nowhere".to_owned(),
//! #             content: Arc::new(Nowhere),
//! #             rules: Arc::new(()),
//! #             roster: vec![Profile { account: ProfileId(1), joined: Tick::ZERO, left: None }],
//! #             seed: Seed(0),
//! #             first: Tick::ZERO,
//! #             origin: None,
//! #             schema: Schema::new("nothing").digest(),
//! #         }
//! #     }
//! # }
//! corvid::app! {
//!     struct Nothing;
//!     const PERIOD: TickSpan = TickSpan::CRADLE;
//!     type State = Still;
//! }
//! ```
//!
//! # The five types, and which one a game starts with
//!
//! A Corvid game is one [`Game`], and a `Game` is five types and a tick span:
//!
//! | | What it says |
//! |---|---|
//! | [`State`] | What a tick is: the state, the level, the rules, the actions, and the one function that advances them |
//! | [`Controller`] | Everything client-local that is not a device: the input declaration, `action`, `look` and what a pad should feel |
//! | `Bot` | A second [`Controller`], for the seats nobody is in |
//! | [`Render`] | What a frame is drawn with: the game's pipelines, and the `draw` that uses them |
//! | [`Auralizer`] | What a frame sounds like: the `hear` that fills an [`AudioFrame`] from a state |
//!
//! A headless run needs only the first. The other four are what a player in
//! front of the run reads, hears and sees, and `()` implements every one of
//! them — which is why the example above is a whole dedicated server whose
//! controller, bot, renderer and ear are one character each.
//!
//! # What is behind it, and what is not
//!
//! The simulation ring, the whole of the client ring — the four contracts, the
//! renderer, the audio frame, the input snapshot, [`color`], [`shape`] and
//! [`camera`] — and the runtime. All of that is reachable here
//! unconditionally.
//!
//! "Unconditionally" is wide on purpose. A camera is fixed-point state, a
//! raycast is integer arithmetic and a colour is four bytes, so none of those
//! needs a feature; neither do [`Render`], [`Target`], `wgpu` and the mesh
//! crates. `Game` names a renderer, so a build of this crate compiles a
//! graphics stack
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
    Player, PlayerId, Presence, ProfileId, SaveSlot, Scope, State, Time,
};
// The filesystem a level is read through, named here rather than forwarded by
// `corvid_behavior`. `Source` is in `Level::load`'s signature and `Malformed`
// is its error, so a game implementing the trait needs both — and this is the
// one crate in the workspace whose job is to save it naming two.
pub use corvid_files::{self as files, Malformed, Missing, Source};
pub use corvid_hash::{self as hash, Digest, Hasher, digest};

// The client-local half: what a player reads, hears and sees.
pub use corvid_control::{self as control, Acting, Controller, LevelRef, Updating};
pub use corvid_sound::{self as sound, AudioFrame, Auralizer, Cue, CueId, Hearing, SoundId};
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
pub use corvid_time::{self as time, Clock, Duration, Elapsed, Tick, TickSpan, Ticks};

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
pub use corvid_render::{self as render, Drawing, Extent, Icon, Opened, Render, Target, wgpu};
#[cfg(feature = "render")]
pub use corvid_ui_render as ui_render;
#[cfg(feature = "window")]
pub use corvid_window::{self as window, Size};

// The console and the tunables, and the encoding a save is written in.
pub use corvid_dev as dev_console;
pub use corvid_wire as wire;
