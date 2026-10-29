//! Where the runtime stands on the network: alone, gathering in a lobby, or
//! playing the lobby's session linked.
//!
//! A game's controller asks with [`NetRequest`]s and reads a [`NetView`];
//! the loop acts on the requests after each displayed frame, before the next
//! tick. Hosting or joining opens a lobby while the session of this machine's
//! own runs on (a title screen stays live); the host's start links every
//! machine into the lobby's session (see `relink.rs`); a tick's
//! [`lobby`](corvid_behavior::Command::lobby) brings everyone back to the
//! lobby at that tick, still connected, onto a fresh session of their own
//! whose level is the one the linked session had come to; leaving says so to
//! the others and plays alone again.

use std::string::ToString;
use std::vec::Vec;

use corvid_behavior::{PlayerId, State};
#[cfg(feature = "net")]
use corvid_control::net::{Heard, Member, Stage};
use corvid_control::net::{NetRequest, NetView};
use corvid_replay::Opening;

#[cfg(feature = "net")]
use crate::lobby::{Browser, Lobby};
use crate::{Error, backend::Backend, game::Game, seating::Seating};

use super::Runtime;
#[cfg(feature = "net")]
use super::{Horizon, Play};

/// The runtime's network stage.
pub(crate) struct Net<S: State> {
    /// What the controller is shown this frame.
    pub(crate) view: NetView,
    /// What the controller asked this frame.
    pub(crate) requests: Vec<NetRequest>,
    /// A tick asked for everyone back to the lobby.
    pub(crate) back: bool,
    /// Whether this machine's own session was resumed from a save since it
    /// last linked: a host's lobby then starts from the saved state.
    pub(crate) resumed: bool,
    #[cfg(feature = "net")]
    pub(super) lobby: Option<Lobby>,
    #[cfg(feature = "net")]
    browser: Option<Browser>,
    /// The opening this machine played alone before it linked, to go back
    /// to, and who it played it with.
    #[cfg_attr(
        not(feature = "net"),
        expect(dead_code, reason = "a machine that cannot link never goes home")
    )]
    home: Option<(Opening<S>, Seating, Vec<PlayerId>)>,
}

impl<S: State> Net<S> {
    pub(crate) fn new() -> Self {
        Self {
            view: NetView {
                online: cfg!(feature = "net"),
                ..NetView::default()
            },
            requests: Vec::new(),
            back: false,
            resumed: false,
            #[cfg(feature = "net")]
            lobby: None,
            #[cfg(feature = "net")]
            browser: None,
            home: None,
        }
    }
}

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// Acts on what the controller asked of the network and on what the
    /// network said, once a frame.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`] for a lobby's session whose roster does not fit its
    /// log, which a host running the same build never sends.
    #[cfg_attr(
        not(feature = "net"),
        expect(
            clippy::unnecessary_wraps,
            reason = "linking a session is what fails here, and that is what `net` adds"
        )
    )]
    pub(super) fn network(&mut self) -> Result<(), Error> {
        let mut requests = std::mem::take(&mut self.net.requests);
        // A resume is the runtime's own business, network or none.
        requests.retain(|request| match request {
            NetRequest::Resume { slot } => {
                self.resume(*slot);
                false
            }
            _ => true,
        });
        #[cfg(not(feature = "net"))]
        {
            if !requests.is_empty() {
                self.net.view.note = Some("online play is not in this build".to_string());
            }
            if self.net.back {
                self.net.back = false;
            }
            Ok(())
        }
        #[cfg(feature = "net")]
        {
            for request in requests {
                self.ask(request);
            }
            if let Some(browser) = self.net.browser.as_mut() {
                browser.poll();
            }
            if let Some(lobby) = self.net.lobby.as_mut() {
                lobby.poll();
                if let Play::Linked(link) = &mut self.play {
                    for (from, frame) in link.lobby_frames() {
                        lobby.hear(from, frame.as_deref());
                    }
                    // Whoever the lobby let in since is who plays that seat.
                    for member in lobby.members() {
                        link.admit(member.peer, member.seat);
                    }
                    // And whoever left, the bot plays for until somebody
                    // joins that seat again.
                    for seat in lobby.released() {
                        link.keep(seat);
                    }
                }
            }
            if self.net.back {
                self.net.back = false;
                self.back_to_lobby();
            }
            let began = self.net.lobby.as_mut().and_then(Lobby::began);
            if let (Some(began), Some(socket)) = (began, self.net.lobby.as_ref().map(Lobby::socket))
            {
                self.link(began, socket)?;
            }
            self.gone();
            self.see();
            Ok(())
        }
    }

    /// One request.
    #[cfg(feature = "net")]
    fn ask(&mut self, request: NetRequest) {
        let game = <G::State as State>::NAME;
        let note = match request {
            NetRequest::Host {
                port,
                seats,
                name,
                local,
            } => {
                if self.net.lobby.is_some() {
                    Some("already in a lobby".to_string())
                } else {
                    match Lobby::host(port, &name, game, seats, local || !self.backend.watched()) {
                        Ok(lobby) => {
                            self.net.lobby = Some(lobby);
                            self.net.browser = None;
                            None
                        }
                        Err(why) => Some(std::format!("cannot host on port {port}: {why}")),
                    }
                }
            }
            NetRequest::Join { address, name } => {
                if self.net.lobby.is_some() {
                    Some("already in a lobby".to_string())
                } else {
                    match Lobby::join(&address, &name, game) {
                        Ok(lobby) => {
                            self.net.lobby = Some(lobby);
                            self.net.browser = None;
                            None
                        }
                        Err(why) => Some(std::format!("cannot join {address}: {why}")),
                    }
                }
            }
            NetRequest::Browse(on) => {
                if !on {
                    self.net.browser = None;
                    None
                } else if self.net.browser.is_some() {
                    None
                } else {
                    match Browser::new(game, !self.backend.watched()) {
                        Ok(browser) => {
                            self.net.browser = Some(browser);
                            None
                        }
                        Err(why) => Some(std::format!("cannot listen for games here: {why}")),
                    }
                }
            }
            NetRequest::Ready(ready) => {
                if let Some(lobby) = self.net.lobby.as_mut() {
                    lobby.set_ready(ready);
                }
                None
            }
            NetRequest::Start => match self.net.lobby.as_mut().map(Lobby::start) {
                Some(true) => None,
                Some(false) => Some("every machine in the lobby has to be ready".to_string()),
                None => Some("not in a lobby".to_string()),
            },
            NetRequest::Leave => {
                if let Some(lobby) = self.net.lobby.take() {
                    lobby.leave();
                }
                self.go_home();
                None
            }
            _ => None,
        };
        if note.is_some() {
            self.net.view.note = note;
        }
    }

    /// A lobby that turned this machine away or lost its host: gone, and
    /// this machine alone again.
    #[cfg(feature = "net")]
    fn gone(&mut self) {
        use crate::lobby::Stage as Lobbying;
        let why = match self.net.lobby.as_ref().map(Lobby::stage) {
            Some(Lobbying::Refused(why)) => Some(std::format!("turned away: {why}")),
            Some(Lobbying::Closed) => Some("the host left".to_string()),
            _ => None,
        };
        if let Some(why) = why {
            self.net.lobby = None;
            self.net.view.note = Some(why);
            self.go_home();
        }
    }

    /// Everyone back to the lobby after a linked session: this machine plays
    /// alone again, on the level the session had come to, still in the lobby.
    #[cfg(feature = "net")]
    fn back_to_lobby(&mut self) {
        let level = std::sync::Arc::clone(self.play.session().levels.current());
        if let Some((opening, _, _)) = self.net.home.as_mut() {
            opening.content = level;
        }
        self.go_home();
        if let Some(lobby) = self.net.lobby.as_mut() {
            lobby.back();
        }
    }

    /// Plays alone again, from the opening played before linking; nothing,
    /// for a machine that never linked.
    #[cfg(feature = "net")]
    fn go_home(&mut self) {
        let Some((opening, seating, bots)) = self.net.home.take() else {
            return;
        };
        let Ok(session) = corvid_replay::Session::new(opening) else {
            return;
        };
        let at = session.first();
        let origin = session.opening.origin();
        self.play = Play::Local(Box::new(session));
        self.seating = seating;
        self.bots = bots;
        self.at = at;
        self.previous = std::sync::Arc::clone(&origin);
        self.current = origin;
        if let Horizon::Recent { marked, kept, .. } = &mut self.horizon {
            *marked = at;
            *kept = None;
        }
    }

    /// Remembers what this machine played alone, the first time it links.
    #[cfg(feature = "net")]
    pub(super) fn remember_home(&mut self) {
        if self.net.home.is_none()
            && let Play::Local(session) = &self.play
        {
            let mut opening = session.opening.clone();
            opening.content = std::sync::Arc::clone(session.levels.current());
            opening.first = corvid_time::Tick::ZERO;
            opening.origin = None;
            self.net.home = Some((opening, self.seating, self.bots.clone()));
        }
    }

    /// Plays on from a save slot, replacing this machine's own session: alone,
    /// or on a host still gathering, whose lobby then starts from it. Refused
    /// on a guest and while linked, where the session is everyone's; a slot
    /// that will not read is said in the view's note, and nothing changes.
    fn resume(&mut self, slot: u16) {
        #[cfg(feature = "net")]
        {
            let why = if matches!(self.play, Play::Linked(_)) {
                Some("a game played together cannot load a save; leave it first")
            } else if self
                .net
                .lobby
                .as_ref()
                .is_some_and(|lobby| !lobby.hosting())
            {
                Some("only the host loads a save for a lobby")
            } else {
                None
            };
            if let Some(why) = why {
                self.net.view.note = Some(why.to_string());
                return;
            }
        }
        let schema = self.play.session().opening.schema;
        let slot_id = corvid_behavior::SaveSlot(slot);
        let (session, state) = match self.saves.read::<G::State>(slot_id, schema) {
            Ok(Some(resumed)) => resumed,
            Ok(None) => {
                self.net.view.note = Some(std::format!("slot {slot} is empty"));
                return;
            }
            Err(why) => {
                tracing::warn!(name: "corvid_app.unresumed", slot, %why, "a save could not be resumed");
                self.net.view.note = Some(std::format!("slot {slot} would not load: {why}"));
                return;
            }
        };
        let at = session.last();
        self.play = super::Play::Local(std::boxed::Box::new(session));
        self.previous = std::sync::Arc::clone(&state);
        self.current = state;
        self.at = at;
        if let super::Horizon::Recent { marked, kept, .. } = &mut self.horizon {
            *marked = at;
            *kept = None;
        }
        self.net.resumed = true;
        self.net.view.resumed = Some(slot);
        tracing::info!(name: "corvid_app.resumed", slot, at = %at, "playing on from a save");
    }

    /// The view the controller sees next frame.
    #[cfg(feature = "net")]
    fn see(&mut self) {
        let view = &mut self.net.view;
        view.browsing = self.net.browser.is_some();
        view.heard = self.net.browser.as_ref().map_or_else(Vec::new, |b| {
            b.found()
                .iter()
                .map(|found| Heard {
                    address: found.address.to_string(),
                    name: found.name.clone(),
                    open: found.open,
                    running: found.running,
                })
                .collect()
        });
        view.open = match (&self.play, self.net.lobby.as_ref()) {
            (Play::Linked(link), Some(lobby)) if lobby.hosting() => {
                link.bots().iter().map(|seat| seat.0).collect()
            }
            _ => Vec::new(),
        };
        match self.net.lobby.as_ref() {
            None => {
                view.stage = Stage::Alone;
                view.hosting = false;
                view.port = None;
                view.seats = 0;
                view.members.clear();
                view.can_start = false;
            }
            Some(lobby) => {
                view.stage = if matches!(self.play, Play::Linked(_)) {
                    Stage::Linked
                } else {
                    Stage::Gathering
                };
                view.hosting = lobby.hosting();
                view.port = lobby.local().map(|at| at.port());
                view.seats = lobby.seats();
                view.can_start = lobby.can_start();
                view.members = lobby
                    .members()
                    .iter()
                    .map(|m| Member {
                        name: m.name.clone(),
                        seat: m.seat.0,
                        ready: m.ready,
                        host: m.peer == corvid_net::PeerId(1),
                        me: m.peer == lobby.me(),
                    })
                    .collect();
            }
        }
    }
}
