//! Starting to play over a lobby's socket, in the middle of a run.
//!
//! A game's title screen runs alone; its lobby, once started, leaves a
//! transport for this thread (see `lobby::handoff`). Before the next tick
//! the loop takes it, opens the session the lobby agreed on, and plays that
//! session linked from its first tick. What was being played alone is let
//! go.
//!
//! The host sends the guests its opening's *terms* -- the level's name, the
//! rules, a seat for everyone, the seed and the schema -- and a digest of
//! its level, not the level itself: every machine in a session loads the
//! same level from its own disk, and a level is far bigger than a lobby
//! frame. A guest whose own level does not match the digest says so and
//! plays on alone rather than joining a session it would desync.

use std::string::String;
use std::sync::Arc;
use std::vec::Vec;

use corvid_behavior::{ProfileId, State};
use corvid_hash::Digest;
use corvid_replay::{Opening, Profile, Seed, Session};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

use crate::lobby::{announce, handoff};
use crate::{Error, backend::Backend, game::Game, seating::Seating};

use super::{Horizon, Play, Runtime};

/// What a host sends its guests to open the session on.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct Terms<S: State> {
    level: String,
    rules: S::Rules,
    roster: Vec<Profile>,
    seed: Seed,
    schema: u64,
    /// The digest of the level every machine is to have loaded.
    content: u64,
}

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Takes a session a lobby on this thread started, if one did.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`] for an opening whose roster does not fit its log,
    /// which a host running the same build as this one never sends.
    pub(super) fn relink(&mut self) -> Result<(), Error> {
        let Some(started) = handoff::take() else {
            return Ok(());
        };
        let here = &self.play.session().opening;
        let opening: Opening<G::State> = if let Some(bytes) = &started.terms {
            let terms: Terms<G::State> = match corvid_wire::decode(bytes) {
                Ok(terms) => terms,
                Err(why) => {
                    tracing::error!(
                        name: "corvid_app.lobby_unreadable",
                        %why,
                        "the host's terms could not be read, so this machine plays on alone",
                    );
                    return Ok(());
                }
            };
            let mine = corvid_hash::digest(&*here.content);
            if mine.to_u64() != terms.content || terms.level != here.level {
                tracing::error!(
                    name: "corvid_app.lobby_other_level",
                    host = %terms.level,
                    here = %here.level,
                    "the host plays a different level from this machine's, so this machine plays on alone",
                );
                return Ok(());
            }
            Opening {
                level: terms.level,
                content: Arc::clone(&here.content),
                rules: Arc::new(terms.rules),
                roster: terms.roster,
                seed: terms.seed,
                first: Tick::ZERO,
                origin: None,
                schema: Digest::from_u64(terms.schema),
            }
        } else {
            let opening = Opening {
                level: here.level.clone(),
                content: Arc::clone(&here.content),
                rules: Arc::clone(&here.rules),
                roster: (0..started.width)
                    .map(|seat| Profile {
                        account: ProfileId(u64::from(seat) + 1),
                        joined: Tick::ZERO,
                        left: None,
                    })
                    .collect(),
                seed: here.seed,
                first: Tick::ZERO,
                origin: None,
                schema: here.schema,
            };
            let terms = Terms::<G::State> {
                level: opening.level.clone(),
                rules: <G::State as State>::Rules::clone(&opening.rules),
                roster: opening.roster.clone(),
                seed: opening.seed,
                schema: opening.schema.to_u64(),
                content: corvid_hash::digest(&*opening.content).to_u64(),
            };
            match corvid_wire::encode(&terms) {
                Ok(bytes) => announce(&*started.transport, &started.guests, bytes),
                Err(why) => tracing::error!(
                    name: "corvid_app.lobby_unencoded",
                    %why,
                    "this machine's terms could not be encoded, so no guest can start",
                ),
            }
            opening
        };
        let session = Session::new(opening).map_err(Error::Shape)?;
        let at = session.first();
        let origin = session.opening.origin();
        let link = crate::net::Link::new(session, started.seat, self.budget, started.transport)
            .seated(started.seats);
        tracing::info!(
            name: "corvid_app.lobby_started",
            seat = started.seat.0,
            seats = started.width,
            "the lobby started; playing its session linked",
        );
        self.play = Play::Linked(Box::new(link));
        self.seating = Seating::Playing(started.seat);
        self.bots.clear();
        self.at = at;
        self.previous = Arc::clone(&origin);
        self.current = origin;
        if let Horizon::Recent { marked, kept, .. } = &mut self.horizon {
            *marked = at;
            *kept = None;
        }
        Ok(())
    }
}
