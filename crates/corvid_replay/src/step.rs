//! One tick of a session, and what it did to the level.
//!
//! Every place that plays a session forward -- a runtime playing alone, a
//! lockstep peer and its rollbacks, a seek, a replay being checked -- plays
//! it through [`step`], so none of them can disagree about when the level
//! changed. A tick asks for changes through its command sink; `step` catches
//! them on their way past, passes everything on to the caller's sink as well,
//! applies the changes in the order they were asked for once the tick is
//! done, and has the state fold the new level in through
//! [`State::load_level`](corvid_behavior::State::load_level).

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use corvid_behavior::{
    AchievementId, Command, ExitCode, Level, LevelEdit, LobbyId, PlayerId, PlayerState,
    PresenceText, SaveSlot, StatId, State, Url,
};
use corvid_hash::Digest;
use corvid_time::Tick;

use crate::Opening;
use crate::timeline::Change;

/// A new level and the changes that made it.
pub type Changed<S> = (Arc<<S as State>::Level>, Vec<Change<LevelEdit<S>>>);

/// A tick played: the state it made and, if it changed the level, the new
/// level and the changes that made it.
#[derive(Debug)]
pub struct Stepped<S: State> {
    /// The state after the tick, with any new level folded in.
    pub state: S,
    /// The level the next tick is played on and the changes that made it, if
    /// this tick changed it.
    pub changed: Option<Changed<S>>,
}

/// The players tick `at` is handed: every seat present then, with its action
/// from `row` (or the idle action).
#[must_use]
pub fn players<S: State>(
    opening: &Opening<S>,
    at: Tick,
    row: impl Fn(PlayerId) -> Option<S::Action>,
) -> Vec<PlayerState<S::Action>> {
    let mut out = Vec::with_capacity(opening.roster.len());
    for (seat, profile) in opening.roster.iter().enumerate() {
        // A roster longer than a `PlayerId` can address has seats no action can
        // be attributed to; stopping is what every player of a session does.
        let Ok(seat) = u16::try_from(seat) else {
            break;
        };
        let id = PlayerId(seat);
        let Some(presence) = profile.presence_at(at) else {
            continue;
        };
        out.push(PlayerState {
            id,
            presence,
            action: row(id).unwrap_or_default(),
        });
    }
    out
}

/// Plays one tick on `level`, and applies what it asked of the level.
pub fn step<S: State>(
    state: S,
    level: &Arc<S::Level>,
    players: &[PlayerState<S::Action>],
    rules: &S::Rules,
    command: &mut impl Command<LevelEdit<S>>,
) -> Stepped<S> {
    let mut tee = Tee {
        inner: command,
        changes: Vec::new(),
    };
    let next = state.tick(level, players, rules, &mut tee);
    let changes = tee.changes;
    if changes.is_empty() {
        return Stepped {
            state: next,
            changed: None,
        };
    }
    let new = apply(level, &changes);
    // Every change refused leaves the very level it started from: nothing
    // changed, and nothing is folded in.
    if Arc::ptr_eq(&new, level) {
        return Stepped {
            state: next,
            changed: None,
        };
    }
    let state = next.load_level(Some(level), &new);
    Stepped {
        state,
        changed: Some((new, changes)),
    }
}

/// A level with changes made to it in order. A change the level refuses is
/// dropped; if every one is, the answer is `level` itself.
pub(crate) fn apply<L: Level>(level: &Arc<L>, changes: &[Change<L::Edit>]) -> Arc<L> {
    let mut made: Option<L> = None;
    for change in changes {
        let base = made.as_ref().unwrap_or(level);
        let next = match change {
            Change::Edit(edit) => base.edit(edit),
            Change::Load(name) => L::load(name),
        };
        if let Ok(next) = next {
            made = Some(next);
        }
    }
    made.map_or_else(|| Arc::clone(level), Arc::new)
}

/// What a tick's mark is: its state's digest, and the level's with it when
/// the tick changed the level, so two machines whose edits went differently
/// disagree at that tick.
#[must_use]
pub fn mark<S: State>(state: &S, changed: Option<&S::Level>) -> Digest {
    let of_state = corvid_hash::digest(state);
    match changed {
        None => of_state,
        Some(level) => {
            corvid_hash::digest(&(of_state.to_u64(), corvid_hash::digest(level).to_u64()))
        }
    }
}

/// A sink that catches the level's changes and passes everything on.
struct Tee<'a, C, E> {
    inner: &'a mut C,
    changes: Vec<Change<E>>,
}

impl<C: Command<E>, E: Clone> Command<E> for Tee<'_, C, E> {
    fn edit(&mut self, edit: E) {
        self.changes.push(Change::Edit(edit.clone()));
        self.inner.edit(edit);
    }

    fn lobby(&mut self) {
        self.inner.lobby();
    }

    fn load(&mut self, name: &str) {
        self.changes.push(Change::Load(String::from(name)));
        self.inner.load(name);
    }

    fn unload(&mut self, name: &str) {
        self.inner.unload(name);
    }

    fn quit(&mut self, code: ExitCode) {
        self.inner.quit(code);
    }

    fn save(&mut self, slot: SaveSlot) {
        self.inner.save(slot);
    }

    fn read(&mut self, slot: SaveSlot) {
        self.inner.read(slot);
    }

    fn screenshot(&mut self) {
        self.inner.screenshot();
    }

    fn invite(&mut self, player: PlayerId) {
        self.inner.invite(player);
    }

    fn join_lobby(&mut self, lobby: LobbyId) {
        self.inner.join_lobby(lobby);
    }

    fn leave_lobby(&mut self) {
        self.inner.leave_lobby();
    }

    fn set_presence(&mut self, presence: PresenceText) {
        self.inner.set_presence(presence);
    }

    fn open_url(&mut self, url: Url) {
        self.inner.open_url(url);
    }

    fn achieve(&mut self, achievement: AchievementId) {
        self.inner.achieve(achievement);
    }

    fn stat(&mut self, id: StatId, value: i64) {
        self.inner.stat(id, value);
    }
}
