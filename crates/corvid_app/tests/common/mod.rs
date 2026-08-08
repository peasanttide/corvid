//! Two games and a temporary directory.
//!
//! [`Tally`] is a complete `State` **and** `Present` implementation that
//! keeps every promise the contracts make, and every part of it is there to be
//! falsifiable by one of the tests:
//!
//! * its action varies with the tick, so a loop that logged an action against
//!   the wrong tick produces a different session rather than the same one;
//! * its action also varies with the wall time `look` has been handed, which is
//!   the only way a clock that is not the app's clock can reach the log;
//! * its state owns a `Vec` built fresh every tick, so a state is a real
//!   allocation rather than a handful of integers and a digest of one covers a
//!   column whose length varies with what the players did;
//! * its rules name the ticks it asks the platform for things on, so a test
//!   picks which requests a run makes without going through an input layer,
//!   which a headless run does not have.
//!
//! [`Leaky`] is the same game with one line changed: its tick reads a counter
//! it has been accumulating in its `Scratch`, which `corvid_behavior` forbids.
//! It is here because the `dev` schedule exists to find exactly that, and a
//! check for a leak needs something that leaks.
//!
//! [`backstop`] is the other half of this module and has nothing to do with
//! games: it is how a test that watches a run from a second thread fails rather
//! than hangs.
//!
//! [`Nudge`] is the bot, and it is one line of opinion: it answers
//! [`Action::Bump`] for every seat it is given, every tick. A run of [`Botted`]
//! therefore reads a column at a time — a seat holding a bump is a seat the
//! runtime filled, and a seat holding the idle action is one nothing did.
//!
//! [`Attendance`] is the third, and it exists because the two above cannot see the
//! loop's *arguments*. Its tick writes down the roster it was handed — every
//! seat, in order, with its presence and with whether its action was this
//! client's — so a run of it is a record of who the loop said was playing and
//! which column this client's action landed in. Its own state holds no tick
//! counter, which is the second thing it is here to demonstrate: with
//! [`App::for_ticks`](corvid_app::App::for_ticks) a run of a fixed length
//! costs a game nothing on the wire.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this module separately, so anything only one of them uses is dead in the others"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "this module is private to each test binary, so pub(crate) and pub are equivalent — pub(crate) is the one rustc's unreachable_pub asks for, and the two lints cannot both be satisfied"
)]

pub(crate) mod backstop;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use corvid_behavior::{
    AchievementId, Command, ExitCode, Extract, Extracting, Player, PlayerId, Presence, ProfileId,
    SaveSlot,
};
use corvid_control::{Acting, Controller, Updating};
use corvid_hash::{Digest, digest};
use corvid_input::{Digital, Input};
use corvid_replay::{Opening, Opens, Profile, Schema, Seed};
use corvid_sound::Auralizer;
use corvid_sound::{Cue, Hearing, Listener, SoundId, Source, SourceId};
use corvid_time::{Duration, Tick};
use corvid_vector::FinePoint;
use serde::{Deserialize, Serialize};

/// The one place this game's actions are named.
pub(crate) mod action {
    corvid_input::action_sets! {
        pub set Playing {
            digital REST;
        }
    }
}

/// How these games name a level: by string, which is what a game with no fixed
/// set of them reaches for and the shape `FromStr` is free on.
pub(crate) type Ref = String;

/// The level every session here opens on.
pub(crate) const FIELD: &str = "field";

/// The slot the game saves into.
pub(crate) const SLOT: SaveSlot = SaveSlot(2);

/// The status the game quits with, chosen so that a run which stopped because
/// [`until`](corvid_app::App::until) said so cannot be mistaken for one that
/// quit.
pub(crate) const FAREWELL: ExitCode = ExitCode(7);

/// The achievement the game asks for, which is a request the runtime does not
/// handle.
pub(crate) const APPLAUSE: AchievementId = AchievementId(1);

/// The voice the tally hums through, and the first of the ones the pips use.
pub(crate) const VOICE: SourceId = SourceId(1);

/// What it hums.
pub(crate) const HUM: SoundId = SoundId(1);

/// What a bump rings.
pub(crate) const CHIME: SoundId = SoundId(2);

/// Authored, immutable within a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Level {
    /// The name the runtime would have loaded this by.
    pub(crate) name: Ref,
}

/// Deterministic tuning every peer agrees on, and the four ticks this game
/// asks the platform for something on.
///
/// The requests are keyed to ticks rather than to actions because a headless
/// run has no device layer: an action comes from `intend`, and `intend`
/// is handed an input snapshot nothing refills. A tick that knows its own
/// number can ask for a save on tick seven without anybody pressing anything.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Rules {
    /// How far one bump moves the tally.
    pub(crate) step: i64,
    /// The tick that asks to quit.
    pub(crate) quit_at: Option<Tick>,
    /// What that tick asks to quit *with*, or [`FAREWELL`] when it says
    /// nothing. It is a setting rather than a constant so that a test can put
    /// either of two statuses first and read the difference.
    pub(crate) quit_with: Option<ExitCode>,
    /// A second status the same tick asks to quit with, right after the first.
    ///
    /// Two `Quit`s out of one tick is a legal thing for a game to return — the
    /// vocabulary says nothing against it, and a game that quits from two
    /// places in one tick writes exactly this — and the sink documents that the
    /// first one wins. Without a game that emits both, that sentence is a
    /// comment on a branch nothing takes.
    pub(crate) then_quit_with: Option<ExitCode>,
    /// The tick that asks for a save.
    pub(crate) save_at: Option<Tick>,
    /// The tick that asks for it back.
    pub(crate) read_at: Option<Tick>,
    /// The tick that asks for an achievement, which nothing here handles.
    pub(crate) cheer_at: Option<Tick>,
    /// The tick that asks for a screenshot.
    pub(crate) snap_at: Option<Tick>,
    /// The tick the client-local half stops the clock on.
    ///
    /// A pause is not a request and does not go through `Command`: it is
    /// `Present::simulating`, which the runtime asks the *view* about. So this
    /// is read by `look` rather than by the tick, and what it moves is a field
    /// of the view. It is a rule only because a headless run has no device
    /// layer to press a key on, which is the reason every other setting here is
    /// a rule too.
    pub(crate) pause_at: Option<Tick>,
    /// How many displayed frames the pause lasts.
    pub(crate) pause_for: u64,
}

impl Rules {
    /// Rules that ask for nothing.
    pub(crate) const fn quiet() -> Self {
        Self {
            step: 3,
            quit_at: None,
            quit_with: None,
            then_quit_with: None,
            save_at: None,
            read_at: None,
            cheer_at: None,
            snap_at: None,
            pause_at: None,
            pause_for: 0,
        }
    }
}

/// Everything that cannot be recomputed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Tally {
    /// The tally.
    pub(crate) count: i64,
    /// Which tick this state is at. A tick is not handed its own number, so a
    /// state that wants one counts.
    pub(crate) now: Tick,
    /// Who bumped on the tick that produced this state.
    ///
    /// The column exists so that the state owns an allocation rather than being
    /// two integers a machine copies without noticing — a run of this game is a
    /// run in which every tick allocates, which is what the retention and
    /// capture tests are measuring the cost of.
    pub(crate) movers: Vec<PlayerId>,
}

/// One player's intent for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Action {
    /// Did nothing, and what every seat but this client's submits.
    #[default]
    Idle,
    /// Move the tally by one step.
    Bump,
}

/// What [`Tally`] carries from tick to tick, and never reads.
///
/// The degenerate memo: written every tick, consulted by nothing, so whatever
/// the runtime does to it — carry it, or throw it away on the `dev` schedule —
/// the states are the same states. That is exactly what makes it useful here.
/// `tests/dev.rs` discards this on a schedule and checks that [`Tally`] agrees
/// with itself anyway, which is only a check at all if there is a real value
/// being discarded; a `Scratch` of `()` would make the honest arm of that test
/// pass by construction.
///
/// It is a counter rather than a pool because there is no longer anywhere for a
/// pool to get its buffers back from: a retiring state is a handle the runtime
/// may not hold the last one of, so [`Tally::tick`] builds its column with
/// [`Vec::new`] and lets it go with the state.
#[derive(Debug, Default)]
pub(crate) struct Odometer {
    /// How many ticks this scratch has been carried through.
    ticks: u64,
}

/// Camera and cosmetics: never hashed, never sent, never rolled back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct View {
    /// How much wall-clock time `look` has been handed.
    ///
    /// [`Tally::intend`] reads it, which `corvid_present` warns is a display
    /// -rate quantity reaching an action. It is deliberate here: it is the one
    /// route by which a clock the app was not given could reach the session,
    /// and `tests/headless.rs` is what walks it.
    pub(crate) elapsed: Duration,
    /// How many times `look` has been called.
    pub(crate) frames: u64,
    /// How many displayed frames have happened since the pause began.
    pub(crate) held: u64,
    /// Whether the simulation is stopped, which is what
    /// [`Present::simulating`] answers with.
    pub(crate) paused: bool,
}

/// The commands this game's rules ask for at `now`.
fn requests(rules: &Rules, now: Tick, command: &mut impl Command<Reference = Ref>) {
    if rules.save_at == Some(now) {
        command.save(SLOT);
    }
    if rules.read_at == Some(now) {
        command.read(SLOT);
    }
    if rules.cheer_at == Some(now) {
        command.achieve(APPLAUSE);
    }
    if rules.snap_at == Some(now) {
        command.screenshot();
    }
    // Last, so that a tick which both asks for something and asks to stop has
    // its other request drained before the loop breaks. The sink takes the
    // whole list either way; the order is what a reader of `Requests` sees.
    if rules.quit_at == Some(now) {
        command.quit(rules.quit_with.unwrap_or(FAREWELL));
        if let Some(second) = rules.then_quit_with {
            command.quit(second);
        }
    }
}

/// The level reads nothing: this fixture's is a constant.
impl corvid_behavior::Level for Level {
    type Reference = Ref;

    fn load(
        _reference: &Ref,
        _files: &dyn corvid_files::Source,
    ) -> Result<Self, corvid_files::Malformed> {
        Ok(Self {
            name: FIELD.to_owned(),
        })
    }
}

impl corvid_behavior::State for Tally {
    const NAME: &'static str = "tally";

    type Level = Level;
    type Rules = Rules;
    type Action = Action;

    fn tick(
        self,
        _level: &Level,
        players: &[Player<'_, Action>],
        rules: &Rules,
        command: &mut impl Command<Reference = Ref>,
    ) -> Self {
        // Fresh every tick. The state owns this column and hands it to whoever
        // holds the state, and nothing gives it back.
        let mut movers = Vec::new();
        let mut count = self.count;
        for player in players {
            if matches!(player.action, Action::Bump) {
                count += rules.step;
                movers.push(player.id);
            }
        }
        requests(rules, self.now, command);
        Self {
            count,
            now: self.now.next(),
            movers,
        }
    }
}

/// The player: what a tick's action is, and the client-local pause.
///
/// This is where `View` went. It holds the elapsed wall clock and the frame
/// count that used to be a `View`, and it is the only thing that writes them —
/// which is what `update` being the one `&mut self` hook buys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Hands {
    /// Simulated seconds handed to `update`.
    pub(crate) elapsed: Duration,
    /// How many displayed frames have gone by.
    pub(crate) frames: u64,
    /// Whether the simulation is held.
    pub(crate) paused: bool,
    /// How many frames it has been held for.
    pub(crate) held: u64,
    /// The tick and rules the last `update` saw, so `action` and `simulating`
    /// can read what only `update` is handed.
    pub(crate) at: Tick,
    /// What the pause is decided against.
    pub(crate) pause_at: Option<Tick>,
    /// And for how long.
    pub(crate) pause_for: u64,
}

/// What a `Hands` is built from: when to pause, and for how long.
///
/// This used to be read off `frame.rules` — the client half was handed the
/// simulation's own tuning. It is a `Config` now, which is the honest place
/// for it: a pause is one machine's, and `Rules` is what every peer agrees on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Holding {
    /// The tick from which to hold, if at all.
    pub(crate) pause_at: Option<Tick>,
    /// How many displayed frames to hold for.
    pub(crate) pause_for: u64,
}

impl Controller<Tally> for Hands {
    type Config = Holding;

    /// A fixture with nothing to press.
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(config: Holding) -> Self {
        Self {
            pause_at: config.pause_at,
            pause_for: config.pause_for,
            ..Self::default()
        }
    }

    fn configure(&mut self, config: Holding) {
        self.pause_at = config.pause_at;
        self.pause_for = config.pause_for;
    }

    /// Bump on one tick in three, counting the simulated seconds `update` has
    /// been handed as though they were ticks.
    ///
    /// The second term is what makes a wall clock visible. Under the app's fake
    /// clock it is a function of the tick number and nothing else, so the
    /// sequence is fixed; under a real clock a run of a few dozen ticks never
    /// reaches one second and the sequence is a different one.
    fn action(&self, acting: Acting<'_, Tally>) -> Action {
        if acting.input.digital(action::REST).held {
            return Action::Idle;
        }
        let phase = acting.state.now.0.wrapping_add(self.elapsed.as_secs());
        if phase.is_multiple_of(3) {
            Action::Bump
        } else {
            Action::Idle
        }
    }

    /// The one writer, and where this game's pause is decided.
    ///
    /// The pause is counted in *displayed frames* rather than in ticks, and it
    /// has to be: once it starts there are no more ticks, so a condition
    /// written against the tick number would never come round again.
    fn update(&mut self, updating: Updating<'_, Tally>) {
        self.elapsed = self.elapsed.saturating_add(updating.dt);
        self.frames += 1;
        self.at = updating.state.now;
        if self.pause_at.is_some_and(|at| updating.state.now >= at) {
            self.held = self.held.saturating_add(1);
            self.paused = self.held <= self.pause_for;
        }
    }

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }

    /// The client-local pause: no tick while this says so, while `update`,
    /// `hear` and the backend carry on.
    fn simulating(&self) -> bool {
        !self.paused
    }
}

/// The ear: one voice per unit of tally, and a chime on a tick that moved it.
///
/// The voice count varies with the state, which is what makes two ticks'
/// captured frames different files rather than the same bytes twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ears {
    /// The newest extracted state's count.
    count: i64,
    /// The one before it, so a change is noticeable.
    was: i64,
    /// Which tick the newest is.
    at: Tick,
}

impl Extract<Tally> for Ears {
    fn extract(&mut self, extracting: Extracting<'_, Tally>) {
        if extracting.state.now != self.at {
            self.was = self.count;
        }
        self.count = extracting.state.count;
        self.at = extracting.state.now;
    }
}

impl Auralizer<Tally> for Ears {
    type Config = ();

    fn new((): ()) -> Self {
        Self::default()
    }

    fn configure(&mut self, (): ()) {}

    fn hear(&mut self, hearing: Hearing<'_>) {
        hearing.out.listen(Listener::new(hearing.camera.pose));

        let voices = u32::try_from(self.count.rem_euclid(5)).unwrap_or(0) + 1;
        for voice in 0..voices {
            hearing
                .out
                .source(Source::new(SourceId(VOICE.0 + voice), HUM).at(FinePoint::ZERO));
        }

        if self.count != self.was {
            let id = hearing.out.next_id(self.at);
            hearing.out.cue(Cue::new(id, CHIME).at(FinePoint::ZERO));
        }
    }
}

/// The drawing half of [`Tally`], for the runs that open a device.
///
/// One triangle in clip space with no camera, no uniform and no depth: the
/// claim `tests/windowless.rs` makes is about a *digest*, so what this has to
/// be is drawable — a mesh a device actually uploads and rasterises, so that
/// the run being compared is a run in which a device did work.
///
/// This is where the view and the pipelines are declared, because `Render` is
/// the base of the client-local half: `Present` reads and writes the view in
/// all three of its functions and declares neither.
impl Extract<Tally> for Painted {
    fn extract(&mut self, _extracting: Extracting<'_, Tally>) {}
}

impl corvid_render::Render<Tally> for Painted {
    type Config = ();

    fn new(opened: corvid_render::Opened<'_>, (): ()) -> Self {
        Self::setup(opened.device, opened.queue, opened.format)
    }

    fn configure(&mut self, (): ()) {}

    fn draw(&mut self, drawing: corvid_render::Drawing<'_, Tally>) {
        use corvid_render::wgpu;

        let target = drawing.target;
        let graphics = self;
        let mut pass = target
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tally"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(&graphics.pipeline);
        graphics.triangle.draw(&mut pass, 0..1);
    }
}

/// Built once, where the device is.
impl Painted {
    fn setup(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        use corvid_render::wgpu;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tally"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tally.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tally"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        Self {
            pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("tally"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(corvid_mesh_render::VERTEX_LAYOUT)],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            }),
            triangle: corvid_mesh_render::upload(&triangle(), device, "tally.triangle"),
        }
    }
}

/// What [`Tally`]'s renderer builds once.
#[derive(Debug)]
pub(crate) struct Painted {
    /// The one pipeline.
    pipeline: wgpu::RenderPipeline,
    /// The one mesh.
    triangle: corvid_mesh_render::Uploaded,
}

/// One triangle, which is the least geometry that is still geometry.
fn triangle() -> corvid_mesh::Mesh {
    use corvid_mesh::{Mesh, Vertex};
    use corvid_vector::OctDirection;
    Mesh::new(
        vec![
            Vertex::new([-Vertex::FULL, -Vertex::FULL, 0], OctDirection::UP),
            Vertex::new([Vertex::FULL, -Vertex::FULL, 0], OctDirection::UP),
            Vertex::new([0, Vertex::FULL, 0], OctDirection::UP),
        ],
        vec![0, 1, 2],
        corvid_fixed::I16F16::from_f64(1.0),
    )
}

/// One player, as the tick was handed them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Seen {
    /// The seat the runtime attributed this action to.
    pub(crate) id: PlayerId,
    /// Where the runtime says this player stands this tick.
    pub(crate) presence: Presence,
    /// Whether the action in this column came from this client's controller
    /// rather than being the default every other seat submits.
    pub(crate) mine: bool,
    /// The bits of the alpha the controller was handed, for the column that
    /// has one.
    pub(crate) alpha: u16,
}

/// What one tick was handed, in the order it was handed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Roll {
    /// One entry per player the tick saw.
    pub(crate) seats: Vec<Seen>,
}

/// Every tick's roster, and nothing else.
///
/// **There is no tick counter here**, and its absence is the point. A run of a
/// fixed length is [`App::for_ticks`](corvid_app::App::for_ticks), and a
/// predicate that wants the tick is handed it, so the number a game used to
/// keep for a test's benefit — hashed, serialized and sent every tick — is not
/// in this state. The index into [`rolls`](Self::rolls) is the tick's offset
/// from the opening, which is a fact about the vector rather than a column.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Attendance {
    /// One entry per tick that has run, in order.
    pub(crate) rolls: Vec<Roll>,
}

/// One player's intent: a record of what `intend` was handed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Mark {
    /// True only for an action `intend` built, so a seat holding
    /// [`Default`] is distinguishable from one this client submitted for.
    pub(crate) mine: bool,
    /// The bits of the frame's alpha at the moment `intend` ran.
    pub(crate) alpha: u16,
}

impl corvid_behavior::State for Attendance {
    const NAME: &'static str = "census";

    type Level = Level;
    type Rules = Rules;
    type Action = Mark;

    fn tick(
        self,
        _level: &Level,
        players: &[Player<'_, Mark>],
        _rules: &Rules,
        _command: &mut impl Command<Reference = Ref>,
    ) -> Self {
        let mut rolls = self.rolls;
        rolls.push(Roll {
            seats: players
                .iter()
                .map(|player| Seen {
                    id: player.id,
                    presence: player.presence,
                    mine: player.action.mine,
                    alpha: player.action.alpha,
                })
                .collect(),
        });
        Self { rolls }
    }
}

/// A profile that joined on the opening tick and has not left.
pub(crate) const fn seat(account: u64) -> Profile {
    Profile {
        account: ProfileId(account),
        joined: Tick::ZERO,
        left: None,
    }
}

/// An opening for [`Attendance`] with the roster given.
///
/// The roster is the argument because that is what the tests using this game
/// vary: how many seats there are, when each of them joined, and which of them
/// this client submits for.
pub(crate) fn attendance(roster: Vec<Profile>) -> Opening<Attendance> {
    Opening {
        level: FIELD.to_owned(),
        content: Arc::new(Level {
            name: FIELD.to_owned(),
        }),
        rules: Arc::new(Rules::quiet()),
        roster,
        seed: Seed(0x5eed),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("census")
            .field("Attendance.rolls", "Vec<Roll>")
            .field("Roll.seats", "Vec<Seen>")
            .field("Seen", "PlayerId | Presence | bool | u16")
            .digest(),
    }
}

/// The description of these types, which a capture records and a load compares.
pub(crate) fn schema() -> Digest {
    Schema::new("tally")
        .field("State.count", "i64")
        .field("State.now", "Tick")
        .field("State.movers", "Vec<PlayerId>")
        .field("Action", "Idle | Bump")
        .digest()
}

/// An opening for either game: one seat, joining on the first tick, with the
/// rules given.
pub(crate) fn opening<S>(rules: Rules) -> Opening<S>
where
    S: corvid_behavior::State<Level = Level, Rules = Rules> + Default,
{
    Opening {
        level: FIELD.to_owned(),
        content: Arc::new(Level {
            name: FIELD.to_owned(),
        }),
        rules: Arc::new(rules),
        roster: vec![Profile {
            account: ProfileId(1000),
            joined: Tick::ZERO,
            left: None,
        }],
        seed: Seed(0x5eed),
        first: Tick::ZERO,
        // `None`, which is `S::default()` — and both fixture states open on
        // theirs, so nothing is lost by not stating it.
        origin: None,
        schema: schema(),
    }
}

/// An input snapshot with the rest button held, which is the one thing a test
/// can say to `intend` when nothing refills the snapshot.
pub(crate) fn resting() -> Input {
    let mut input = Input::new(action::SETS);
    input.set_digital(action::REST, Digital::HELD);
    input
}

/// The digest of a state, spelled out so a test reads as an assertion about
/// states rather than about hashing.
pub(crate) fn mark(state: &Tally) -> Digest {
    digest(state)
}

/// A directory under the system's temporary one, removed when this is dropped.
///
/// Written here rather than taken from a crate because it is twenty lines and
/// because a capture test wants to know exactly which paths exist — a helper
/// that hid the path would be hiding the thing under test.
#[derive(Debug)]
pub(crate) struct Scratchpad {
    /// Where it is.
    path: PathBuf,
}

impl Scratchpad {
    /// A directory nothing else is using, named for the test that asked.
    ///
    /// It is not created here. `App::capture` is what creates a capture
    /// directory, and a test that found one already there would not be testing
    /// that.
    pub(crate) fn new(what: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("corvid_app-{}-{what}-{unique}", std::process::id()));
        drop(fs::remove_dir_all(&path));
        Self { path }
    }

    /// Where it is.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratchpad {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}

/// Where a run of [`Tally`] that was told nothing else starts.
///
/// Every test here says [`App::opening`](corvid_app::App::opening) for itself,
/// because what a test varies is the rules: which tick quits, which tick saves,
/// which tick asks for a screenshot. This is here because a
/// [`Game`](corvid_app::Game) names a state that can open a session on its own,
/// and the quiet rules are the honest answer for a run nobody has configured.
impl Opens for Tally {
    fn opening() -> Opening<Self> {
        opening(Rules::quiet())
    }
}

/// The same, for the roster fixture: one seat, joined on the first tick.
impl Opens for Attendance {
    fn opening() -> Opening<Self> {
        attendance(vec![seat(1000)])
    }
}

/// The game the tests in this crate play.
///
/// Written out rather than generated, and it is five lines because a game is
/// five types: the tally simulates, [`Hands`] plays it, nothing bots for it,
/// [`Painted`] draws it and [`Ears`] hears it.
///
/// [`CRADLE`](corvid_time::TickSpan::CRADLE) is the period because it is the
/// rate every timed assertion in these tests was written against — a marker
/// that chose another one would change how many ticks a run of a fixed duration
/// simulates, which is a different run and a different digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counting;

impl corvid_app::Game for Counting {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Tally;
    type Controller = Hands;
    type Bot = ();
    type Render = Painted;
    type Auralizer = Ears;
}

/// The tally with nobody playing it: a dedicated server, as a game.
///
/// Four `()`s, which is what a game that reads no device, runs no bot, opens no
/// adapter and opens no sound card writes. It is here for the settings tests,
/// whose subject is the *document* rather than what is in it — four configs of
/// `()` are four fields that still have to be named, written down and read
/// back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bare;

impl corvid_app::Game for Bare {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Tally;
    type Controller = ();
    type Bot = ();
    type Render = ();
    type Auralizer = ();
}

/// The tally with a bot in it, which is the game the bot tests play.
///
/// Its controller is `()` and its [`Bot`](corvid_app::Game::Bot) is [`Nudge`],
/// which is what makes a run of it readable as a column at a time: every
/// non-idle action in the log came from a bot, because the only other thing
/// writing one answers [`Action::Idle`] and a row nobody wrote holds the same.
///
/// Nothing is drawn and nothing is heard, because what these tests are about is
/// which seats got filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Botted;

impl corvid_app::Game for Botted {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Tally;
    type Controller = ();
    type Bot = Nudge;
    type Render = ();
    type Auralizer = ();
}

/// The bot for [`Tally`]: a bump, every tick, for whatever seat it is asked
/// about.
///
/// Being *distinguishable* is the whole requirement. [`Action::Idle`] is what a
/// row nobody wrote holds and what the `()` controller answers, so an
/// unconditional [`Action::Bump`] is the one answer that separates "a bot
/// played this seat" from "nothing did".
///
/// It ignores [`Acting::seat`], which is the honest thing for a bot with one
/// opinion to do: what the seat is for is telling several apart, and this one
/// plays them all the same.
///
/// `REAL` is false, which is what a controller with nobody behind it says. It
/// is about the platform rather than about whether the controller runs, and
/// this one is asked for an action every tick of every seat it plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nudge;

impl Controller<Tally> for Nudge {
    type Config = ();

    const REAL: bool = false;
    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Tally>) -> Action {
        Action::Bump
    }

    fn update(&mut self, _updating: Updating<'_, Tally>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

/// The game the roster tests play: [`Attendance`], with nothing drawn and
/// nothing heard.
///
/// A marker of its own beside [`Counting`] rather than a parameter on it. What
/// these tests are about is the *arguments* a tick was handed — which seats
/// were in the roster, and which column this client's action landed in — and a
/// run of them opens no device and makes no sound, so the two types that would
/// have to be `()` for one game and real for the other are written as `()`
/// here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attending;

impl corvid_app::Game for Attending {
    const PERIOD: corvid_time::TickSpan = corvid_time::TickSpan::CRADLE;

    type State = Attendance;
    type Controller = Marker;
    type Bot = ();
    type Render = ();
    type Auralizer = ();
}

/// The controller for [`Attendance`]: it marks every action as its own.
///
/// The old fixture also recorded the alpha its `intend` was handed, and there
/// is no alpha to record any more — a controller's `action` runs once per tick
/// and never sees an interpolation weight, because interpolation is the
/// renderer's and happens in a shader. So the column is written as zero and the
/// test that read it says why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Marker;

impl Controller<Attendance> for Marker {
    type Config = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Attendance>) -> Mark {
        Mark {
            mine: true,
            alpha: 0,
        }
    }

    fn update(&mut self, _updating: Updating<'_, Attendance>) {}

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}
