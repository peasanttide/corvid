//! Where a run opens: the game's own opening, a save, a recording, or a level
//! a command line named.
//!
//! The seam against `settings.rs` is failure. Every call here can refuse --
//! a save that will not read, a level this game does not have -- which is why
//! they are not part of the chain the builder calls make.

use std::sync::Arc;

use corvid_behavior::{Level, PlayerId, State};
use corvid_replay::{Opening, Session};

use crate::app::{App, Error, Started};
use crate::cli::{Argument, Arguments, Load};
use crate::game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig};
use crate::saves::Saves;

impl<G: Game> App<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    /// The session this run plays, and where in it the run starts.
    ///
    /// A run that was told to resume plays the session it was handed rather
    /// than the one the opening would have started, and it opens at that
    /// session's last tick rather than at its first. A run that was told
    /// neither opens the game's own opening, and the second half of the answer
    /// is [`None`] -- there is nothing to resume, and the opening's origin state
    /// is where it starts.
    ///
    /// [`load`](Self::load) beats [`replay`](Self::replay), because a slot is
    /// the more specific of the two.
    pub(super) fn open(
        &mut self,
        opening: Opening<G::State>,
        saves: &Saves,
    ) -> Result<Started<G>, Error> {
        let schema = opening.schema;
        let resumed = match (self.load.take(), self.replay.take()) {
            (Some(slot), _) => saves
                .read::<G::State>(slot, schema)?
                .ok_or(Error::Empty { slot })?,
            (None, Some(path)) => crate::saves::recorded::<G::State>(&path, schema)?,
            (None, None) => return Ok((Session::new(opening).map_err(Error::Shape)?, None)),
        };
        let (session, state) = resumed;
        let at = session.last();
        Ok((session, Some((at, state))))
    }

    /// The builder calls [`arguments`](Self::arguments) stands for, made at the
    /// last possible moment.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`](crate::Error::Argument) carrying [`Argument::UnreadableLevel`] for a
    /// `--level` whose name this game's loader refuses, and [`Error::Unopened`](crate::Error::Unopened)
    /// for a `--level` on an app that has no opening to name a level in. Both
    /// come from [`open_on`](Self::open_on), which is the only call here that
    /// can fail.
    pub(super) fn apply(mut self, arguments: Arguments) -> Result<Self, Error> {
        if arguments.headless {
            self = self.headless();
        }
        if let Some(ticks) = arguments.ticks {
            self = self.for_ticks(ticks);
        }
        if let Some(path) = arguments.record {
            self = self.record(path);
        }
        if let Some(directory) = arguments.state {
            self = self.state(directory);
        }
        // The seat first, so that `--spectator --seat 1` watches the seat it
        // was told to. `--seat 0` and a command line that says nothing are the
        // same value, so a builder that chose a seat keeps it either way --
        // seat zero is what both sides default to, and there is nothing in the
        // parsed arguments that could tell the two apart.
        if arguments.seat != PlayerId(0) {
            self = self.seat(arguments.seat);
        }
        if arguments.spectator {
            self = self.spectating();
        }
        if arguments.num_bots > 0 {
            self = self.bots(arguments.num_bots);
        }
        match arguments.load {
            Some(Load::Save(slot)) => self = self.load(slot),
            Some(Load::Demo(path)) => self = self.replay(path),
            Some(Load::Level(json)) => self = self.open_on(&json)?,
            None => {}
        }
        Ok(self)
    }

    /// Opens on the level `name` refers to rather than on the one the game's
    /// opening does.
    ///
    /// Both halves of the opening move: the
    /// [`level`](corvid_replay::Opening::level) name a session records, and the
    /// [`content`](corvid_replay::Opening::content) a tick is handed. The
    /// second is what makes this a flag that opens on a level rather than one
    /// that renames the level a run is already on -- the name is hashed into
    /// nothing, so a `--level` that moved only it would change what the session
    /// claims and not a byte of what it plays.
    ///
    /// # What the game's loader is given, and what it is not
    ///
    /// The name and nothing else, because
    /// [`Level::load`](corvid_behavior::Level::load) takes nothing else. A game
    /// whose levels are self-describing opens on the one named, content and
    /// all; a game that reads its levels from somewhere it cannot reach is
    /// **refused**, with what its own loader said. That is the honest pair of
    /// answers: the alternative is a flag that appears to choose and does not.
    ///
    /// # Errors
    ///
    /// [`Error::Argument`](crate::Error::Argument) carrying [`Argument::UnreadableLevel`] if the game's
    /// loader refuses the name, and [`Error::Unopened`](crate::Error::Unopened) if there is no opening
    /// to name a level in.
    pub(super) fn open_on(mut self, name: &str) -> Result<Self, Error> {
        let content = <<G::State as State>::Level as Level>::load(name).map_err(|why| {
            Error::Argument(Argument::UnreadableLevel {
                value: name.to_owned(),
                why: why.to_string(),
            })
        })?;
        let opening = self.opening.as_mut().ok_or(Error::Unopened)?;
        name.clone_into(&mut opening.level);
        opening.content = Arc::new(content);
        Ok(self)
    }
}
