//! One displayed frame: extract, hear, draw, and say where the run has got to.
//!
//! The seam against `advance.rs` is that nothing here can move the simulation.
//! A frame reads the two states the loop already holds and the weight between
//! them, and no machine has to agree with any other about when one happens.

use std::sync::Arc;

use corvid_behavior::{ExitCode, Extract, Extracting};
use corvid_control::{Controller, Updating};
use corvid_fixed::Factor16;
use corvid_hash::digest;
use corvid_sound::{Auralizer, Hearing};
use corvid_time::{Duration, Time};

use crate::backend::{Backend, Frame};
use crate::game::Game;
#[cfg(feature = "net")]
use crate::runtime::Play;
use crate::runtime::Runtime;
use crate::{Error, Outcome, Progress};

impl<G: Game, B: Backend<G>> Runtime<G, B> {
    /// One displayed frame: advance the view, extract the sound, hand the
    /// frame to the backend.
    ///
    /// `dt` is the same interval the [`Step`] was advanced by, which is the
    /// only wall-clock quantity either half of a game ever sees and is the one
    /// [`look`](corvid_control::Controller::look) is specified to take.
    pub(super) fn display(&mut self, alpha: Factor16, dt: Duration) -> Result<(), Error> {
        // Three views of one instant, so one frame is built and cloned twice
        // rather than three being built from the same fields. A clone is four
        // atomic increments and no copy of a state, which is what a `Frame` of
        // handles buys and is why this is not worth writing any other way.
        //
        // An owned frame borrows nothing, so building one is an ordinary method
        // call and the three calls below are free to take `&mut self` in
        // whatever order they like.
        let time = self.now();
        self.controller.update(Updating {
            state: &self.current,
            input: &self.input,
            loading: None,
            time,
            dt,
            seat: self.seating.watched(),
        });
        self.persist_settings();
        let camera = self.controller.look();

        // Extracted once per displayed frame, for the settled newest state --
        // never once per replayed tick. The renderer holds the pair and the
        // shader lerps between them with `alpha`.
        let state = Arc::clone(&self.current);
        let level = Arc::clone(&self.play.session().opening.content);
        let extracting = Extracting {
            state: &*state,
            level: &*level,
            time,
            player: Some(self.seating.watched()),
        };
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.extract(extracting);
        }
        self.ear.extract(extracting);

        self.audio.clear();
        self.ear.hear(&mut self.audio, Hearing::new(camera, time));
        // Nothing about the two states crosses this seam. The renderer already
        // holds whatever `extract` put in it; what goes over is the weight
        // between them.
        self.backend.present(
            Frame {
                at: self.at,
                camera,
                loading: None,
                time,
                alpha,
                audio: &self.audio,
            },
            self.graphics.as_mut(),
        )
    }

    /// Where the session is: the tick, and the wall clock since it opened.
    ///
    /// The elapsed half is zero for now. The loop's clock is a fixed step and
    /// what it accumulates is a debt of ticks rather than a duration since the
    /// opening, so there is nothing here to report yet that would not be a
    /// number this function invented.
    pub(super) const fn now(&self) -> Time {
        Time {
            tick: self.at,
            elapsed: core::time::Duration::ZERO,
        }
    }

    /// Publishes where the run has got to, if anybody asked to be told.
    pub(super) fn publish(&self, finished: bool) {
        if let Some(emitter) = &self.progress {
            emitter.set(Progress {
                tick: self.at,
                // The trace answers for every tick the loop advanced, which is
                // the normal path and a lookup rather than a hash. Computing
                // one is the fallback for a caller that replaced the trace,
                // where the state in hand is still the truth about `at`.
                mark: self
                    .play
                    .session()
                    .marks
                    .get(self.at)
                    .unwrap_or_else(|| digest(&*self.current)),
                frames: self.backend.frames(),
                finished,
            });
        }
    }

    /// Writes down what the run is asked to leave behind, and hands it back.
    ///
    /// The capture's last two files and the `--record` file are written here
    /// and not before, because both hold the *whole* session and a session is
    /// only whole once the last tick has run. A run asked for both writes the
    /// same bytes twice, into a capture's `session` and into the named file, so
    /// either is a file a `--demo` opens.
    pub(super) fn finish(self, exit: ExitCode) -> Result<Outcome<G>, Error> {
        #[cfg(feature = "net")]
        let traffic = match &self.play {
            Play::Local(_) => crate::Traffic::default(),
            Play::Linked(link) => link.totals(),
        };
        let session = self.play.into_session();
        if let Some(capture) = self.backend.capture() {
            let bytes = session.save().map_err(|why| Error::Encoded {
                what: "a session",
                why,
            })?;
            capture.close(&bytes, &session.marks)?;
        }
        if let Some(path) = &self.record {
            crate::record::write(path, &session)?;
        }
        Ok(Outcome {
            session,
            state: self.current,
            exit,
            requests: self.sink.into_requests(),
            #[cfg(feature = "net")]
            traffic,
        })
    }
}
