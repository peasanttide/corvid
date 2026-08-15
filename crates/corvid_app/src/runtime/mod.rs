//! The loop: what happens per tick, what happens per displayed frame, and
//! where the boundary between them is.

use std::{path::PathBuf, sync::Arc};

use corvid_behavior::PlayerId;
use corvid_control::Controller as _;
use corvid_input::Input;
use corvid_signal::Emitter;
use corvid_sound::AudioFrame;
use corvid_time::Tick;

use crate::{
    Error, Progress, Retention, Settings, app::Stop, backend::Backend, commands::Sink, game::Game,
    saves::Saves, seating::Seating,
};

mod advance;
mod display;
mod drive;
mod plan;
mod saves;

pub(crate) use plan::Plan;

use plan::{Flow, Horizon, Play, Ticked};

/// Everything the loop reads and writes.
///
/// One struct rather than a pile of locals, so that the per-tick half and the
/// per-frame half are two functions rather than one long one. The two states
/// are handles because that is what a [`Frame`] holds: building one is four
/// atomic increments and no copy of a state, which is what makes it affordable
/// to build one per call rather than once per displayed frame.
pub(crate) struct Runtime<G: Game, B> {
    /// The session being played, which is the run's whole output, and whoever
    /// is playing it.
    play: Play<G::State>,
    /// Which seat this client looks through, and whether it plays it.
    ///
    /// Everything that is about *watching* -- the controller's `update`, its
    /// camera, and so the renderer and the ear -- reads
    /// [`watched`](Seating::watched), which is always a seat. The one thing
    /// that reads [`playing`](Seating::playing) is the write into the log, and
    /// the call that decides what to write.
    seating: Seating,
    /// The game's caches, carried from tick to tick.
    /// The state at [`at`](Self::at) minus one.
    previous: Arc<G::State>,
    /// The state at [`at`](Self::at).
    current: Arc<G::State>,
    /// Which tick [`current`](Self::current) is.
    at: Tick,
    /// The client-local half's state, moved only by `look`.
    /// Who is playing, and where they are looking.
    controller: G::Controller,
    /// What plays the seats nobody is in.
    ///
    /// One instance for the whole run whatever [`bots`](Self::bots) holds,
    /// because [`Acting::seat`] is how a bot tells the seats apart: a runtime
    /// that built one per seat would have decided for the game that they are
    /// independent, and a game whose bots differ says so in its own config.
    bot: G::Bot,
    /// The seats [`bot`](Self::bot) answers for, in roster order.
    bots: Vec<PlayerId>,
    /// What is drawn with, or [`None`] on a run that opened no device.
    ///
    /// An `Option` rather than a bare renderer because a renderer is built
    /// against a device and a headless run has none -- so "there is no renderer"
    /// is the honest thing to hold rather than one built from nothing.
    graphics: Option<G::Render>,
    /// The ear.
    ear: G::Auralizer,
    /// What the devices say, as of the last reading. This is the frame's input,
    /// and `look` is what reads it.
    input: Input,
    /// What no tick has spent yet: every edge and every displacement since the
    /// last tick, folded together. `action` is what reads it.
    ///
    /// [`None`] for a run nobody refills, where the snapshot is whatever
    /// [`App::input`](crate::App::input) was given and is deliberately the same
    /// for every tick of the run. A windowed run fills it in
    /// [`refill`](Self::refill), because a window ends the edge interval once
    /// per displayed frame while the loop consumes it once per tick, and the
    /// two rates are not the same rate.
    unspent: Option<Input>,
    /// What the devices say, frame by frame, where a caller is standing in for
    /// a player. [`None`] for a windowed run, which is refilled by the window,
    /// and for a run whose snapshot never changes.
    /// The one audio frame, kept for the life of the run and refilled per
    /// frame.
    audio: AudioFrame,
    /// Where a displayed frame goes.
    backend: B,
    /// What the ticks asked the platform for.
    sink: Sink,
    /// Where a save is written and read.
    saves: Saves,
    /// The directory the settings file is written into, which is the one this
    /// game keeps everything else in too.
    root: PathBuf,
    /// Where the session is written when the run ends, if anywhere.
    record: Option<PathBuf>,
    /// When to stop, if the caller said.
    stop: Option<Stop<G::State>>,
    /// The tick to stop *before*, if the caller asked for a fixed number of
    /// them. A count rather than a predicate, because the predicate is checked
    /// after a tick has run and `for_ticks(Ticks::NONE)` has to mean no ticks at
    /// all.
    deadline: Option<Tick>,
    /// Where to publish progress, if the caller said.
    progress: Option<Emitter<Progress>>,
    /// How far back the session is kept.
    horizon: Horizon<G::State>,
    /// What the player has set, as it stands. Compared against what the
    /// controller answers after every displayed frame, and written down when
    /// the two differ.
    settings: Settings<G>,
}

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Builds the loop's state from a session that is already at its opening.
    ///
    /// A [`Frame`] holds two states and there is only one before the first
    /// tick, so both ends of the run's opening pair are the same handle: two
    /// increments of one refcount rather than two clones of the origin state,
    /// and the state itself is the one the opening already holds. The
    /// alternative -- a `previous` that is an [`Option`] -- would put a branch in
    /// front of every extractor for the sake of the first frame.
    ///
    /// # Errors
    ///
    /// Only a linked run has one: [`Error::Halted`](crate::Error::Halted) if the state a `--load` or
    /// a `--demo` resumed at is one the peer will not adopt, which is a tick
    /// outside the session it was handed.
    /// Writes the settings file when the controller says its config has moved.
    ///
    /// [`Controller::config`](corvid_control::Controller::config) answers
    /// [`None`] unless a controller edits its own config, which almost none do --
    /// so the ordinary cost of this per displayed frame is one method call
    /// returning nothing. Only a controller that answered [`Some`] pays for the
    /// comparison, and only a controller whose answer *changed* pays for the
    /// write.
    ///
    /// A refused write is dropped rather than ending a run that is otherwise
    /// fine: a full disk is a reason not to keep somebody's new key binding and
    /// not a reason to stop the game they are playing. It is reported, because a
    /// setting that silently did not save is the failure this is not allowed to
    /// have.
    pub(super) fn persist_settings(&mut self) {
        let Some(config) = self.controller.config() else {
            return;
        };
        if config == self.settings.controls {
            return;
        }
        self.settings.controls = config;
        if let Err(why) = self.settings.save(&self.root) {
            tracing::warn!(
                name: "corvid_app.unsaved",
                %why,
                "the player's settings changed and could not be written down",
            );
        }
    }

    #[cfg_attr(
        not(feature = "net"),
        expect(
            clippy::unnecessary_wraps,
            reason = "seating a peer is what fails here, and that is what `net` adds; a signature that changed with the feature would make every caller carry the same cfg"
        )
    )]
    pub(crate) fn new(
        mut plan: Plan<G::State>,
        backend: B,
        controller: G::Controller,
        bot: G::Bot,
        graphics: Option<G::Render>,
        ear: G::Auralizer,
        settings: Settings<G>,
    ) -> Result<Self, Error> {
        let resumed = plan.resumed.take();
        let (at, state) = resumed
            .clone()
            .unwrap_or_else(|| (plan.session.first(), plan.session.opening.origin()));
        let previous = Arc::clone(&state);
        let current = state;
        let horizon = match plan.retention {
            Retention::Everything => Horizon::Everything,
            Retention::Recent { ticks } => Horizon::Recent {
                window: ticks,
                marked: at,
                kept: None,
            },
        };

        // Who is playing. A transport is the whole of the difference, and the
        // resumed state is handed to the peer rather than only to the display
        // -- a peer opening on the origin while the display showed a loaded
        // save would send digests for a session nobody else is in.
        #[cfg(feature = "net")]
        let playing = match plan.transport.take() {
            Some(transport) => {
                // The watched seat, whether or not it is played: a peer is a
                // seat's place in the session, and a spectator watching a seat
                // somebody else fills is one that submits nothing and folds in
                // what arrives for it.
                let mut link = Box::new(crate::net::Link::new(
                    plan.session,
                    plan.seating.watched(),
                    plan.budget,
                    transport,
                ));
                if let Some((at, state)) = resumed {
                    link.adopt(at, <G::State>::clone(&state))?;
                }
                Play::Linked(link)
            }
            None => Play::Local(Box::new(plan.session)),
        };
        #[cfg(not(feature = "net"))]
        let playing = {
            drop(resumed);
            Play::Local(Box::new(plan.session))
        };

        Ok(Self {
            controller,
            bot,
            bots: plan.bots,
            graphics,
            ear,
            play: playing,
            seating: plan.seating,
            previous,
            current,
            at,
            input: plan.input,
            unspent: None,
            audio: AudioFrame::new(),
            backend,
            sink: Sink::default(),
            saves: plan.saves,
            root: plan.root,
            record: plan.record,
            stop: plan.stop,
            deadline: plan.deadline,
            progress: plan.progress,
            horizon,
            settings,
        })
    }
}
