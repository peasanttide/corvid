//! A guest's half of a lobby: saying hello once the host answers, keeping
//! the room as the host describes it, and starting when the host does.

use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::PeerId;

use super::say::{Say, Seen};
use super::{HOST, Lobby, Member, Stage, Started};

impl Lobby {
    pub(super) fn guest_poll(
        &mut self,
        heard: Vec<(PeerId, Say)>,
        joined: &[PeerId],
        lost: &[PeerId],
    ) {
        if !self.greeted && joined.contains(&HOST) {
            self.greeted = true;
            let hello = Say::Hello {
                name: self.name.clone(),
                game: self.game.clone(),
            };
            self.say(HOST, &hello);
        }
        for (from, said) in heard {
            if from != HOST {
                continue;
            }
            match said {
                Say::Room { members, seats } => self.mirror(members, seats),
                Say::Refused { why } => self.stage = Stage::Refused(why),
                Say::Start { terms } => self.started(terms),
                Say::Leave => self.stage = Stage::Closed,
                _ => {}
            }
        }
        if lost.contains(&HOST) && self.stage == Stage::Gathering {
            self.stage = Stage::Closed;
        }
    }

    /// Takes the room as the host describes it, and greets the other guests
    /// so their datagrams reach this machine once the session starts.
    fn mirror(&mut self, members: Vec<Seen>, seats: u16) {
        self.seats = seats;
        self.members = members
            .into_iter()
            .map(|seen| Member {
                peer: PeerId(seen.peer),
                name: seen.name,
                seat: PlayerId(seen.seat),
                ready: seen.ready,
                address: seen.address,
            })
            .collect();
        let net = &self.net;
        for m in &self.members {
            if m.peer == self.me || m.peer == HOST || net.address(m.peer).is_some() {
                continue;
            }
            if let Some(address) = m.address.as_deref()
                && let Err(why) = net.connect(m.peer, address)
            {
                tracing::warn!(name: "corvid_app.lobby_unreached", peer = %m.peer, %why, "another guest could not be reached");
            }
        }
    }

    fn started(&mut self, terms: Vec<u8>) {
        let seat = self
            .members
            .iter()
            .find(|m| m.peer == self.me)
            .map_or(PlayerId(0), |m| m.seat);
        self.began = Some(Started {
            seat,
            seats: self.seat_map(),
            width: self.seats,
            terms: Some(terms),
            guests: Vec::new(),
        });
        self.stage = Stage::Linked;
    }
}
