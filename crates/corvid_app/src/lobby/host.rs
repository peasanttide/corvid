//! The host's half of a lobby: seating whoever says hello, telling everyone
//! who is in, and shouting on the local network.

use std::string::ToString;
use std::vec::Vec;

use corvid_behavior::PlayerId;
use corvid_net::PeerId;

use super::beacon::Shout;
use super::say::{Say, Seen};
use super::{HOST, Lobby, Member};

impl Lobby {
    pub(super) fn host_poll(&mut self, heard: &[(PeerId, Say)], lost: &[PeerId]) {
        let mut changed = false;
        for (from, said) in heard {
            match said {
                Say::Hello { name, game } => changed |= self.admit(*from, name, game),
                Say::Ready { ready } => {
                    if let Some(m) = self.members.iter_mut().find(|m| m.peer == *from) {
                        m.ready = *ready;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        for peer in lost {
            let before = self.members.len();
            self.members.retain(|m| m.peer != *peer);
            changed |= self.members.len() != before;
        }
        if changed {
            self.tell_room();
        }
        let open = self
            .seats
            .saturating_sub(u16::try_from(self.members.len()).unwrap_or(u16::MAX));
        let port = self.local().map_or(0, |at| at.port());
        let shout = Shout {
            game: self.game.clone(),
            name: self.name.clone(),
            port,
            open,
        };
        if let Some(shouter) = self.shouter.as_mut() {
            shouter.shout(&shout);
        }
    }

    /// Seats a guest that said hello, or says why not. Answers whether the
    /// room changed.
    fn admit(&mut self, from: PeerId, name: &str, game: &str) -> bool {
        if self.members.iter().any(|m| m.peer == from) {
            return false;
        }
        let why = if game != self.game {
            Some(std::format!(
                "this lobby is playing {}, not {game}",
                self.game
            ))
        } else if from == HOST || from.is_none() {
            Some("that peer number is taken; try again".to_string())
        } else {
            None
        };
        let free = (0..self.seats)
            .map(PlayerId)
            .find(|seat| self.members.iter().all(|m| m.seat != *seat));
        match (why, free) {
            (Some(why), _) => {
                self.say(from, &Say::Refused { why });
                false
            }
            (None, None) => {
                let why = "every seat is taken".to_string();
                self.say(from, &Say::Refused { why });
                false
            }
            (None, Some(seat)) => {
                let address = self
                    .net
                    .as_ref()
                    .and_then(|net| net.address(from))
                    .map(|at| at.to_string());
                self.members.push(Member {
                    peer: from,
                    name: name.to_string(),
                    seat,
                    ready: false,
                    address,
                });
                true
            }
        }
    }

    fn tell_room(&self) {
        let members: Vec<Seen> = self
            .members
            .iter()
            .map(|m| Seen {
                peer: m.peer.0,
                name: m.name.clone(),
                seat: m.seat.0,
                ready: m.ready,
                address: m.address.clone(),
            })
            .collect();
        let room = Say::Room {
            members,
            seats: self.seats,
        };
        for m in self.members.iter().filter(|m| m.peer != HOST) {
            self.say(m.peer, &room);
        }
    }
}
