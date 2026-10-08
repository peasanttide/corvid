//! Starting to play over a lobby's socket, in the middle of a run.
//!
//! A game's title screen runs alone; its lobby, once started, is taken by
//! the loop's network stage (see `net.rs`) before the next tick, which opens
//! the session the lobby agreed on and plays it linked from its first tick
//! over the lobby's socket. What was being played alone is kept, to go back
//! to.
//!
//! The host sends the guests its opening's *terms* -- the level's name, the
//! rules, a seat for everyone, the seed and the schema -- and a digest of
//! its level, not the level itself: every machine in a session loads the
//! same level from its own disk, and a level is far bigger than a lobby
//! frame. A guest whose own level does not match the digest says so and
//! plays on alone rather than joining a session it would desync. A guest
//! built with another schema than the host's -- another build, whose
//! types or pace differ -- leaves the lobby, telling the host, and is told
//! why (`OTHER_BUILD`).

use std::string::String;
use std::sync::Arc;
use std::vec::Vec;

use corvid_behavior::LevelEdit;
use corvid_behavior::{ProfileId, State};
use corvid_hash::Digest;
use corvid_replay::{Changes, Opening, Profile, Seed, Session};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

use crate::lobby::{Shared, Started, announce};
use crate::{Error, backend::Backend, game::Game, seating::Seating};

use super::{Horizon, Play, Runtime};

/// Why a guest turns down a host built with another schema.
const OTHER_BUILD: &str = "that lobby runs a different build";

/// What a host sends its guests to open the session on.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct Terms<S: State> {
    level: String,
    rules: S::Rules,
    roster: Vec<Profile>,
    seed: Seed,
    schema: u64,
    /// The digest of the level every machine opened on.
    content: u64,
    /// What the host's session did to that level since, which the new
    /// session opens on the result of.
    changes: Changes<LevelEdit<S>>,
    /// The tick the session opens on: the opening's for a fresh one, the
    /// saved tick for a host that resumed a save before starting.
    first: Tick,
    /// The state it opens on, encoded, for a host that resumed a save; a
    /// fresh session opens on the game's own default.
    origin: Option<Vec<u8>>,
}

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Takes a session a lobby on this thread started, if one did.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`] for an opening whose roster does not fit its log,
    /// which a host running the same build as this one never sends.
    pub(super) fn link(
        &mut self,
        started: Started,
        socket: Arc<corvid_net_udp::UdpNet>,
    ) -> Result<(), Error> {
        let session = self.play.session();
        let here = &session.opening;
        let origin = session.levels.origin();
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
            if terms.schema != here.schema.to_u64() {
                tracing::error!(
                    name: "corvid_app.lobby_other_build",
                    host = terms.schema,
                    here = here.schema.to_u64(),
                    "the host runs a build with another schema, so this machine leaves its lobby",
                );
                if let Some(lobby) = self.net.lobby.as_mut() {
                    lobby.refuse(OTHER_BUILD);
                }
                return Ok(());
            }
            let mine = corvid_hash::digest(&**origin);
            if mine.to_u64() != terms.content || terms.level != here.level {
                tracing::error!(
                    name: "corvid_app.lobby_other_level",
                    host = %terms.level,
                    here = %here.level,
                    "the host plays a different level from this machine's, so this machine plays on alone",
                );
                return Ok(());
            }
            let content = Arc::clone(
                corvid_replay::Timeline::rebuild(Arc::clone(origin), &terms.changes).current(),
            );
            let state = match terms.origin.as_deref().map(corvid_wire::decode::<G::State>) {
                None => None,
                Some(Ok(state)) => Some(Arc::new(state)),
                Some(Err(why)) => {
                    tracing::error!(
                        name: "corvid_app.lobby_unreadable_state",
                        %why,
                        "the saved state the host started from could not be read, so this machine plays on alone",
                    );
                    return Ok(());
                }
            };
            Opening {
                level: terms.level,
                content,
                rules: Arc::new(terms.rules),
                roster: terms.roster,
                seed: terms.seed,
                first: terms.first,
                origin: state,
                schema: Digest::from_u64(terms.schema),
            }
        } else {
            self.host_opening(&started, &socket)
        };
        // What was played alone is remembered only once a session is to be
        // played linked, so a guest that turned the host down keeps playing.
        self.remember_home();
        let session = Session::new(opening).map_err(Error::Shape)?;
        let at = session.first();
        let origin = session.opening.origin();
        let transport = Box::new(Shared(socket));
        let link = crate::net::Link::new(session, started.seat, self.budget, transport)
            .seated(started.seats)
            .with_bots(started.bots);
        let link = if started.joining {
            link.joining()
        } else {
            link
        };
        let link = if started.keeps { link.keeping() } else { link };
        // A save resumed for this lobby is in its session now.
        self.net.resumed = false;
        self.net.view.resumed = None;
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

    /// The opening a host starts its lobby's session on: the level this
    /// machine has come to, a seat for everyone -- and, for a host that
    /// resumed a save while gathering, the saved state at the saved tick.
    /// Its terms go to every guest and are kept for a machine that joins
    /// later.
    fn host_opening(
        &mut self,
        started: &Started,
        socket: &corvid_net_udp::UdpNet,
    ) -> Opening<G::State> {
        let session = self.play.session();
        let here = &session.opening;
        let origin = session.levels.origin();
        let (first, state) = if self.net.resumed {
            (self.at, Some(Arc::clone(&self.current)))
        } else {
            (Tick::ZERO, None)
        };
        let opening = Opening {
            level: here.level.clone(),
            content: Arc::clone(session.levels.current()),
            rules: Arc::clone(&here.rules),
            roster: (0..started.width)
                .map(|seat| Profile {
                    account: ProfileId(u64::from(seat) + 1),
                    joined: first,
                    left: None,
                })
                .collect(),
            seed: here.seed,
            first,
            origin: state,
            schema: here.schema,
        };
        let terms = Terms::<G::State> {
            level: opening.level.clone(),
            rules: <G::State as State>::Rules::clone(&opening.rules),
            roster: opening.roster.clone(),
            seed: opening.seed,
            schema: opening.schema.to_u64(),
            content: corvid_hash::digest(&**origin).to_u64(),
            changes: session.levels.changes().clone(),
            first,
            origin: opening
                .origin
                .as_deref()
                .and_then(|state| corvid_wire::encode(state).ok()),
        };
        match corvid_wire::encode(&terms) {
            Ok(bytes) => {
                if let Some(lobby) = self.net.lobby.as_mut() {
                    lobby.keep_terms(bytes.clone());
                }
                announce(socket, &started.guests, bytes);
            }
            Err(why) => tracing::error!(
                name: "corvid_app.lobby_unencoded",
                %why,
                "this machine's terms could not be encoded, so no guest can start",
            ),
        }
        opening
    }
}
