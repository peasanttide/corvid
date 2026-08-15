//! One tick over the wire: submit, collect, and what a datagram did.
//!
//! The seam against the three files beside this one is the tick. Everything
//! here happens once per tick whatever the session is doing; the rest is what
//! happens when peers stop agreeing.

use corvid_behavior::State;
use corvid_lockstep::Datagram;
use corvid_net::{Channel, Delivery, PeerId};

use crate::net::{Control, Link, TickTraffic, Transfer, halted, refused};

impl<S: State> Link<S> {
    /// One tick: submit, receive, advance, send.
    ///
    /// [`None`] is a client that plays nobody: nothing is submitted, and a peer
    /// receives, folds in, predicts and simulates whether or not it has spoken.
    /// What goes out is still a datagram, because acknowledging what this
    /// machine has heard is what keeps everyone else's window reaching back far
    /// enough to catch it up.
    ///
    /// # What a spectator over a transport does not yet do
    ///
    /// **Somebody else has to be sitting in the seat it watches, and
    /// [`Budget::delay`](corvid_lockstep::Budget) has to be greater than
    /// zero.** Nothing here enforces either, because the case that works is the
    /// case this is for -- watching a seat a real peer plays -- and a guard would
    /// forbid it along with the rest.
    ///
    /// If the watched seat is *empty*, the session stops rather than plays on.
    /// [`Frontier::agreed`](corvid_lockstep::Frontier::agreed) is the minimum
    /// over live seats of what each has confirmed, counting a seat that has
    /// confirmed nothing as [`Tick::ZERO`], and
    /// [`Peer::advance`](corvid_lockstep::Peer::advance) declines to simulate
    /// past `agreed() + Budget::ahead` -- so a column nobody ever writes pins the
    /// frontier at [`Tick::ZERO`] and every peer in the session stalls after
    /// `ahead` ticks, **this one included**. Zero rather than the session's
    /// first tick, which is worse than it sounds for a resumed session: a run
    /// that opened at tick nine hundred stalls `ahead` ticks past *zero*, so it
    /// stalls on the tick it opened on. What answers that is somebody
    /// filling the seat: a machine, a bot, or a
    /// [`Peer::depart`](corvid_lockstep::Peer::depart) retiring it.
    ///
    /// And a spectator is not quite silent, whatever it submits.
    /// [`Peer::outgoing`](corvid_lockstep::Peer::outgoing) falls back to the
    /// session's first tick for a seat that has confirmed nothing, and a
    /// datagram fills a row the log does not hold with
    /// [`Action::default`](Default::default) -- so one confirmed idle row for
    /// the watched seat goes out every tick. At `delay > 0` that row is the
    /// same default on every machine and costs nothing. At `delay == 0` the
    /// real peer in that seat writes a real action at exactly that tick, and
    /// two different confirmed values for one seat is
    /// [`Halt::Contradiction`](corvid_lockstep::Halt), which ends the session
    /// rather than stalling it.
    ///
    /// The sink is the caller's, for the reason it is everywhere
    /// else in this workspace -- a rollback simulates with it, so it cannot be
    /// borrowed from the loop for the length of the call -- and the commands
    /// come back because [`Peer::take_commands`](corvid_lockstep::Peer::take_commands)
    /// holds what the ticks simulated for the first time asked for.
    ///
    /// # Errors
    ///
    /// [`Error::Log`](crate::Error::Log) if this machine's own action could not
    /// be recorded, and [`Error::Diverged`](crate::Error::Diverged) or
    /// [`Error::Halted`](crate::Error::Halted) for what a peer answers when the
    /// session cannot continue. **A packet that will not decode is neither**:
    /// it is counted and dropped, because a socket carries whatever is sent to
    /// it and a run that stopped on the first stray byte would be a run
    /// anybody could stop.
    pub(crate) fn play(
        &mut self,
        action: Option<S::Action>,
        command: &mut impl corvid_behavior::Command,
    ) -> Result<(), crate::Error> {
        let mut traffic = TickTraffic::default();

        // This machine's own intent, for `now + Budget::delay`. It goes in
        // before the sending below, so the datagram this tick puts on the wire
        // already carries it.
        if let Some(action) = action {
            self.peer.submit(action).map_err(refused)?;
        }

        self.collect(&mut traffic)?;

        // Sent before simulating, which is the order `corvid_lockstep`'s own
        // worked example uses: what goes out is this seat's newest actions, and
        // they are all decided by now. Sending afterwards would put this tick's
        // simulation between the decision and the announcement of it for no
        // gain -- the datagram carries no state.
        self.broadcast(&mut traffic);

        traffic.advanced = self.peer.advance(command).map_err(halted)?;

        // Stalling is ordinary: a peer declines to simulate whenever it is
        // ahead of what every seat has confirmed, and the next datagram ends
        // it. What is not ordinary is stalling for rows that no longer exist --
        // a peer whose frontier is more than a window behind the newest head
        // anybody has announced is waiting for rows that have fallen out of
        // every window still being sent, and no amount of waiting will do. It
        // says so, and whichever machine answers sends a state.
        if self.is_stuck() {
            self.say_all(Control::Stuck {
                seat: self.peer.seat().0,
                agreed: self.peer.frontier.agreed(),
            });
            tracing::warn!(
                name: "corvid_app.stuck",
                agreed = %self.peer.frontier.agreed(),
                heard = %self.heard_head,
                "the actions this machine is waiting for are older than any window                  still carries; asking for a state",
            );
        }

        self.traffic = traffic;
        self.totals.fold(traffic);
        Ok(())
    }

    /// Polls the transport and folds every datagram in, deepest rollback kept.
    pub(super) fn collect(&mut self, traffic: &mut TickTraffic) -> Result<(), crate::Error> {
        // Taken out of the transport's borrow first. `poll` hands each arrival
        // to a closure that borrows the bytes for the length of the call, and
        // what happens to a datagram here is a rollback that borrows the peer --
        // so the copy is what lets the two happen one after the other rather
        // than inside each other.
        self.inbox.clear();
        let inbox = &mut self.inbox;
        // What this poll turned up that is not a datagram, acted on after the
        // borrow ends: the sink borrows the transport, and everything below
        // borrows the peer or sends something.
        let mut gone: Vec<PeerId> = Vec::new();
        let mut heard: Vec<(PeerId, Control)> = Vec::new();
        let mut arrived: Vec<PeerId> = Vec::new();
        // Paired with the peer that sent it, because who sent a state is half
        // of whether to adopt it. See `rescue`.
        let mut transferred: Vec<(PeerId, Transfer<S>)> = Vec::new();
        self.transport.poll(&mut |from, delivery| match delivery {
            Delivery::Datagram(bytes) => inbox.push(bytes.to_vec()),
            Delivery::Stream {
                channel: Channel::Control,
                bytes,
            } => match corvid_wire::decode::<Control>(bytes) {
                Ok(control) => heard.push((from, control)),
                Err(why) => tracing::warn!(
                    name: "corvid_app.unreadable_control",
                    peer = %from,
                    why = %why,
                    "a control frame this session could not read; dropped",
                ),
            },
            Delivery::Stream {
                channel: Channel::Transfer,
                bytes,
            } => match corvid_wire::decode::<Transfer<S>>(bytes) {
                Ok(transfer) => transferred.push((from, transfer)),
                Err(why) => tracing::warn!(
                    name: "corvid_app.unreadable_transfer",
                    peer = %from,
                    why = %why,
                    "a state transfer this session could not read; dropped",
                ),
            },
            // The other reliable channels carry an opening and a state
            // transfer, and this runtime transfers no state -- so a frame on one
            // is somebody else's traffic and saying so is all that can honestly
            // be done with it.
            Delivery::Stream { channel, bytes } => tracing::debug!(
                name: "corvid_app.unread_stream",
                peer = %from,
                channel = %channel,
                bytes = bytes.len(),
                "this runtime reads no reliable channel yet, so this frame is dropped",
            ),
            Delivery::Joined => {
                arrived.push(from);
                tracing::info!(
                    name: "corvid_app.peer_joined", peer = %from, "a peer is reachable",
                );
            }
            Delivery::Lost { because } => {
                gone.push(from);
                tracing::warn!(
                    name: "corvid_app.peer_lost",
                    peer = %from,
                    why = %because,
                    "a peer went away; its seat submits nothing from here on and this                      machine stops waiting for it",
                );
            }
            // `Delivery` is `#[non_exhaustive]`, so a backend built against a
            // later version of `corvid_net` may hand over something this
            // runtime has never heard of. Noting it and carrying on is the only
            // honest answer: a lockstep session's correctness rests on the
            // datagrams above, and a delivery kind that did not exist when this
            // was written cannot be one of them.
            other => tracing::debug!(
                name: "corvid_app.unknown_delivery",
                peer = %from,
                what = ?other,
                "a delivery this runtime has no handling for; dropped",
            ),
        });

        // A machine that has gone is a machine whose actions will never arrive,
        // and a peer that kept waiting for them would stall for the rest of the
        // session -- which is the failure that looks like the game freezing and
        // reports nothing.
        //
        // **The tick is proposed rather than decided.** Far enough ahead that
        // no machine has confirmed past it -- a peer runs at most `delay + ahead`
        // beyond what every seat has spoken for -- so an ordinary departure
        // costs no rollback at all; and it is folded in by `Peer::depart`,
        // which keeps the earliest, so two machines proposing different ticks
        // land on the same one.
        self.agree(&gone, &heard, &arrived, traffic)?;
        if let Some(transfer) = self.solicited(transferred) {
            self.rescue(transfer, traffic)?;
        }

        // Out of `self` for the loop, and back at the end: folding a datagram
        // in takes `&mut self.peer`, and the buffer it is being read out of is
        // a field of the same struct. What the round trip preserves is the
        // outer allocation, which is the one made once per run.
        let mut inbox = std::mem::take(&mut self.inbox);
        for bytes in &inbox {
            let datagram: Datagram<S::Action> = match corvid_wire::decode(bytes) {
                Ok(datagram) => datagram,
                Err(why) => {
                    // Counted rather than fatal, and said at `debug` rather than
                    // `warn`: on an open socket this is ordinary. Anything may
                    // send anything to a port.
                    traffic.undecodable = traffic.undecodable.saturating_add(1);
                    tracing::debug!(
                        name: "corvid_app.undecodable",
                        bytes = bytes.len(),
                        why = %why,
                        "a datagram this session could not read; dropped",
                    );
                    continue;
                }
            };
            let rolled = self.peer.receive(&datagram).map_err(halted)?;
            traffic.heard = traffic.heard.saturating_add(1);
            let newest = datagram.head();
            if newest > self.heard_head {
                self.heard_head = newest;
            }
            if rolled.ticks > traffic.rolled.ticks {
                traffic.rolled = rolled;
            }
        }
        inbox.clear();
        self.inbox = inbox;
        Ok(())
    }
}
