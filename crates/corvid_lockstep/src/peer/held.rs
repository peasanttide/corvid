//! What a tick asked the runtime for, held until the tick is final.
//!
//! A tick simulated on a prediction is simulated again once the actions it
//! guessed at arrive, and the second time is the one that happened. So what
//! a tick asks for is written down every time it is simulated, over what it
//! asked the time before, and handed to the runtime's sink only once every
//! seat has confirmed that tick's row: once, from the tick as it finally ran.
//! A request only a prediction made never reaches the runtime, and one a
//! prediction missed -- another machine's `lobby`, landing on a tick this one
//! had guessed idle -- reaches it from the rollback that corrected the guess.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use corvid_behavior::{
    AchievementId, Command, ExitCode, LobbyId, PlayerId, PresenceText, SaveSlot, StatId, Url,
};

/// One request, as the tick made it.
#[derive(Debug)]
enum Asked<E> {
    Edit(E),
    Lobby,
    Load(String),
    Unload(String),
    Quit(ExitCode),
    Save(SaveSlot),
    Read(SaveSlot),
    Screenshot,
    Invite(PlayerId),
    JoinLobby(LobbyId),
    LeaveLobby,
    SetPresence(PresenceText),
    OpenUrl(Box<Url>),
    Achieve(AchievementId),
    Stat(StatId, i64),
}

/// Everything one simulation of one tick asked for, in order.
#[derive(Debug)]
pub(crate) struct Held<E>(Vec<Asked<E>>);

impl<E> Default for Held<E> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<E> Held<E> {
    /// Whether the tick asked for nothing, which is almost every tick.
    pub(crate) const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Hands every request to `sink`, in the order the tick made them.
    pub(crate) fn tell(self, sink: &mut impl Command<E>) {
        for asked in self.0 {
            match asked {
                Asked::Edit(edit) => sink.edit(edit),
                Asked::Lobby => sink.lobby(),
                Asked::Load(name) => sink.load(&name),
                Asked::Unload(name) => sink.unload(&name),
                Asked::Quit(code) => sink.quit(code),
                Asked::Save(slot) => sink.save(slot),
                Asked::Read(slot) => sink.read(slot),
                Asked::Screenshot => sink.screenshot(),
                Asked::Invite(player) => sink.invite(player),
                Asked::JoinLobby(lobby) => sink.join_lobby(lobby),
                Asked::LeaveLobby => sink.leave_lobby(),
                Asked::SetPresence(presence) => sink.set_presence(presence),
                Asked::OpenUrl(url) => sink.open_url(*url),
                Asked::Achieve(achievement) => sink.achieve(achievement),
                Asked::Stat(id, value) => sink.stat(id, value),
            }
        }
    }
}

impl<E> Command<E> for Held<E> {
    fn edit(&mut self, edit: E) {
        self.0.push(Asked::Edit(edit));
    }

    fn lobby(&mut self) {
        self.0.push(Asked::Lobby);
    }

    fn load(&mut self, name: &str) {
        self.0.push(Asked::Load(String::from(name)));
    }

    fn unload(&mut self, name: &str) {
        self.0.push(Asked::Unload(String::from(name)));
    }

    fn quit(&mut self, code: ExitCode) {
        self.0.push(Asked::Quit(code));
    }

    fn save(&mut self, slot: SaveSlot) {
        self.0.push(Asked::Save(slot));
    }

    fn read(&mut self, slot: SaveSlot) {
        self.0.push(Asked::Read(slot));
    }

    fn screenshot(&mut self) {
        self.0.push(Asked::Screenshot);
    }

    fn invite(&mut self, player: PlayerId) {
        self.0.push(Asked::Invite(player));
    }

    fn join_lobby(&mut self, lobby: LobbyId) {
        self.0.push(Asked::JoinLobby(lobby));
    }

    fn leave_lobby(&mut self) {
        self.0.push(Asked::LeaveLobby);
    }

    fn set_presence(&mut self, presence: PresenceText) {
        self.0.push(Asked::SetPresence(presence));
    }

    fn open_url(&mut self, url: Url) {
        self.0.push(Asked::OpenUrl(Box::new(url)));
    }

    fn achieve(&mut self, achievement: AchievementId) {
        self.0.push(Asked::Achieve(achievement));
    }

    fn stat(&mut self, id: StatId, value: i64) {
        self.0.push(Asked::Stat(id, value));
    }
}
