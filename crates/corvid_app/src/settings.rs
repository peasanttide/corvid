//! What the player has set, and where it is kept between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    Error,
    game::{AuralizerConfig, BotConfig, ControllerConfig, Game, RenderConfig},
};

/// What the file is called, inside a game's own state directory.
///
/// Beside `saves/` and the binding file, which is what
/// [`root`](crate::saves::root) resolves and what `--state DIR` redirects:
/// everything one game keeps between runs is one directory, so a player copies
/// one path and a test redirects one flag.
const FILE: &str = "setting.json";

/// Everything the player has set, as one document.
///
/// The four configs together, because they are one thing to a person: a
/// resolution, a volume and a key binding are all "settings", and a game with
/// four files for them is a game with four files to back up and four chances
/// to disagree about which run they belong to.
///
/// # Why JSON, and why this file is the one text format
///
/// It is a **file a player edits**, which is the whole of what this workspace
/// spends a text format on — the same argument the binding table is written
/// under. Everything a peer or a replay reads is `corvid_wire`, because a varint
/// is not something to hand somebody a text editor for; this is the other case.
///
/// # The derives, and the bound they invent
///
/// Every field is a [`Data`](corvid_behavior::Data), so all four are `Clone`,
/// `Debug`, `Eq`, `Serialize` and `Deserialize` whatever game this is. The
/// derives ask for `G` itself to be each of those anyway, which is what a derive
/// does with a type parameter and is wrong in shape rather than in effect: a
/// game is a marker with no fields, which is the one shape that derives all four
/// in a line, and this document's contents do not depend on `G` for anything but
/// their types. `#[derive(Clone, Copy, Debug, PartialEq, Eq)]` on the marker is
/// what a game writes.
///
/// [`Default`] is the one that could not be derived at all, for the reason
/// below: what has to be `Default` is the configs and not the game.
///
/// # A missing key is the default rather than a refusal
///
/// Every field is `#[serde(default)]`, so a file with three of the four keys in
/// it reads back with the fourth at its default and the other three as the
/// player left them. That matters because this document *grows*: a game that
/// adds a setting, or a version of this crate that adds a config, would
/// otherwise turn every existing player's file into
/// [`Error::Setting`](crate::Error::Setting) — which refuses to start a run
/// over a key nobody could have written.
///
/// The cost is in the deserialize bound: filling a field in needs its
/// [`Default`], so [`Deserialize`] asks for all four. That is the same clause
/// [`App::new`](crate::App::new) and [`load`](Self::load) already carry, so
/// there is no game that can start a run and cannot read its own settings.
///
/// What is still refused is a key whose *value* is the wrong shape. A colour
/// where a number belongs is a file somebody edited wrongly, and a run that
/// quietly reset it would be a run that discarded what they meant to say.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "",
    deserialize = "ControllerConfig<G>: Default, BotConfig<G>: Default, \
                   RenderConfig<G>: Default, AuralizerConfig<G>: Default"
))]
pub struct Settings<G: Game> {
    /// What the controller is built from.
    #[serde(default)]
    pub controls: ControllerConfig<G>,
    /// What the bot is built from.
    ///
    /// One config for however many seats a run fills with bots, because a bot
    /// is a setting of the run rather than of a seat: a game whose bots differ
    /// from one another says so in the config, which is the game's own type.
    #[serde(default)]
    pub bot: BotConfig<G>,
    /// What the renderer is built from, once there is a device to build it
    /// against.
    #[serde(default)]
    pub graphics: RenderConfig<G>,
    /// What the sound card is built from.
    #[serde(default)]
    pub audio: AuralizerConfig<G>,
}

/// The defaults, which is what a fresh install has.
///
/// Written out rather than derived: a derive would bound `G` on `Default` when
/// what has to be `Default` is the four *configs*, which is the same `where`
/// clause [`App`](crate::App)'s own `Default` carries.
impl<G: Game> Default for Settings<G>
where
    ControllerConfig<G>: Default,
    BotConfig<G>: Default,
    RenderConfig<G>: Default,
    AuralizerConfig<G>: Default,
{
    fn default() -> Self {
        Self {
            controls: ControllerConfig::<G>::default(),
            bot: BotConfig::<G>::default(),
            graphics: RenderConfig::<G>::default(),
            audio: AuralizerConfig::<G>::default(),
        }
    }
}

impl<G: Game> Settings<G> {
    /// Where this game's settings file is, given the directory it keeps its
    /// files in.
    ///
    /// The root is what `--state DIR` names and what
    /// [`App`](crate::App) resolves for a run that did not — so this is a join
    /// rather than a lookup, and there is one answer to "where does this game
    /// write" rather than one per kind of file.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(FILE)
    }

    /// Reads the file, or answers the defaults where there is none.
    ///
    /// A file that is there and cannot be read is an error rather than a shrug:
    /// starting with the defaults would silently discard whatever the player had
    /// set, and the failure they would see is every control unbound with nothing
    /// saying why.
    ///
    /// A **missing key** is not that. Every field defaults, so a file written by
    /// a build with fewer settings in it reads back with the new one at its
    /// default and the rest as the player left them; what is refused is a key
    /// whose value is the wrong shape. The type's own documentation argues the
    /// split.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] if the file is there and could not be read, and
    /// [`Error::Setting`] if it is there and is not this game's settings.
    pub fn load(root: &Path) -> Result<Self, Error>
    where
        ControllerConfig<G>: Default,
        BotConfig<G>: Default,
        RenderConfig<G>: Default,
        AuralizerConfig<G>: Default,
    {
        let path = Self::path(root);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(why) => return Err(Error::Read { path, why }),
        };
        serde_json::from_str(&text).map_err(|why| Error::Setting { path, why })
    }

    /// Writes the file, creating the directory above it.
    ///
    /// # Errors
    ///
    /// [`Error::Wrote`] if the directory could not be made or the file could not
    /// be written, and [`Error::Encoded`] if the settings could not be written
    /// down at all.
    pub fn save(&self, root: &Path) -> Result<(), Error> {
        let path = Self::path(root);
        let text = serde_json::to_string_pretty(self).map_err(|why| Error::Setting {
            path: path.clone(),
            why,
        })?;
        if let Some(directory) = path.parent() {
            create(directory)?;
        }
        std::fs::write(&path, text).map_err(|why| Error::Wrote { path, why })
    }
}

/// Makes `directory`, reporting where it failed.
fn create(directory: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(directory).map_err(|why| Error::Wrote {
        path: directory.to_path_buf(),
        why,
    })
}
