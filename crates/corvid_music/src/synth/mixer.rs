//! The synthesizer: sixteen channels, a pool of voices, and a block of samples.

use alloc::vec::Vec;

use crate::synth::bank::Bank;
use crate::synth::channel::{Channel, velocity_gain};
use crate::synth::gens;
use crate::synth::midi::{MidiEvent, TimedEvent};
use crate::synth::voice::{Voice, Waveform};

/// How many voices sound at once before the oldest is taken.
const MAX_VOICES: usize = 64;
/// How long a stolen voice takes to fade, in seconds.
const STEAL: f32 = 0.005;

/// A synthesizer: MIDI in, interleaved stereo out.
///
/// It owns no device. [`render`](Self::render) fills a buffer the caller
/// provides, so whether that buffer goes to a sound card, a file, or a mixer
/// that also has footsteps in it is somebody else's decision.
///
/// With a [`Bank`] it plays the bank. Without one it plays a
/// [`Waveform`] per channel, which is enough to hear that the harmony works and
/// nowhere near enough to hear the music -- see the crate's scope for what that
/// buys and what it costs.
///
/// ```
/// use corvid_music::{MidiEvent, Synth};
///
/// let mut synth = Synth::new(48_000);
/// synth.send(MidiEvent::NoteOn { channel: 0, key: 60, velocity: 100 });
///
/// let mut block = vec![0.0f32; 2 * 480];
/// synth.render(&mut block);
///
/// assert_eq!(synth.active_voices(), 1);
/// assert!(block.iter().any(|sample| sample.abs() > 0.01));
/// assert!(block.iter().all(|sample| sample.abs() <= 1.0));
/// ```
#[derive(Clone, Debug)]
pub struct Synth {
    sample_rate: u32,
    bank: Option<Bank>,
    channels: [Channel; 16],
    voices: Vec<Voice>,
    queue: Vec<TimedEvent>,
    clock: u64,
    gain: f32,
}

impl Synth {
    /// A synthesizer with no bank, rendering at `sample_rate` hertz.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
            bank: None,
            channels: [Channel::default(); 16],
            voices: Vec::new(),
            queue: Vec::new(),
            clock: 0,
            gain: 0.25,
        }
    }

    /// A synthesizer playing `bank`.
    #[must_use]
    pub fn with_bank(sample_rate: u32, bank: Bank) -> Self {
        Self {
            bank: Some(bank),
            ..Self::new(sample_rate)
        }
    }

    /// Sets the gain applied to the whole mix before it is clamped.
    ///
    /// A quarter by default, which leaves a seven-voice bar of loud notes below
    /// full scale without the mix having to be limited. The clamp is still
    /// there, so the promise that every sample is in `-1.0 ..= 1.0` holds
    /// whatever this is set to.
    #[must_use]
    pub const fn with_gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    /// The rate it renders at.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The bank it plays, if it has one.
    #[must_use]
    pub const fn bank(&self) -> Option<&Bank> {
        self.bank.as_ref()
    }

    /// Gives it a bank, or takes one away.
    ///
    /// Voices already sounding from the old bank finish rather than being cut,
    /// because a bank swapped between two blocks is a load and not a mistake --
    /// but a sampled voice whose sample has gone finishes at once, which is
    /// audible as a note ending early rather than as a click.
    pub fn set_bank(&mut self, bank: Option<Bank>) {
        self.bank = bank;
    }

    /// Sets the waveform a channel uses when there is no bank.
    pub fn set_waveform(&mut self, channel: u8, waveform: Waveform) {
        if let Some(slot) = self.channels.get_mut(usize::from(channel)) {
            slot.waveform = waveform;
        }
    }

    /// How many voices are sounding.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// The frame the next [`render`](Self::render) starts on.
    #[must_use]
    pub const fn clock(&self) -> u64 {
        self.clock
    }

    /// Applies `event` at the current frame.
    pub fn send(&mut self, event: MidiEvent) {
        self.apply(event);
    }

    /// Queues `event` for the frame it names.
    ///
    /// An event in the past is applied at the start of the next block rather
    /// than dropped, so a caller that queued a bar and then took too long over
    /// it hears a late note instead of a missing one.
    pub fn schedule(&mut self, event: TimedEvent) {
        let at = self.queue.partition_point(|held| held.frame <= event.frame);
        self.queue.insert(at, event);
    }

    /// Queues every event in `events`.
    pub fn schedule_all(&mut self, events: impl IntoIterator<Item = TimedEvent>) {
        for event in events {
            self.schedule(event);
        }
    }

    /// Silences everything and empties the queue, keeping the bank and the
    /// channel settings.
    pub fn reset(&mut self) {
        self.voices.clear();
        self.queue.clear();
    }

    /// Renders the next block into `out`, which is interleaved stereo.
    ///
    /// `out` is overwritten rather than added to. Every sample is in
    /// `-1.0 ..= 1.0`: the mix is scaled by the gain and then clamped, which is
    /// a hard clip and will distort if the gain is set high enough to need it.
    /// A clip is a decision a caller can hear and act on; a sample outside the
    /// range is one that arrives at a device as a click nobody can trace.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let mut done = 0usize;
        while done < frames {
            while self
                .queue
                .first()
                .is_some_and(|event| event.frame <= self.clock)
            {
                let event = self.queue.remove(0);
                self.apply(event.event);
            }
            let until = self
                .queue
                .first()
                .map_or(frames, |event| {
                    let ahead = event.frame.saturating_sub(self.clock);
                    done.saturating_add(usize::try_from(ahead).unwrap_or(frames))
                        .min(frames)
                })
                .max(done + 1)
                .min(frames);
            let span = until - done;
            if let Some(block) = out.get_mut(done * 2..until * 2) {
                self.fill(block);
            }
            self.clock = self.clock.saturating_add(u64::try_from(span).unwrap_or(0));
            done = until;
        }
        for sample in out.iter_mut() {
            *sample = (*sample * self.gain).clamp(-1.0, 1.0);
        }
    }

    /// Mixes every voice into one span, and drops the ones that have finished.
    fn fill(&mut self, out: &mut [f32]) {
        let Self {
            bank,
            channels,
            voices,
            ..
        } = self;
        for voice in voices.iter_mut() {
            let pcm = match bank.as_ref() {
                Some(bank) => match voice.sample_index() {
                    Some(index) => match bank.samples.get(index) {
                        Some(sample) => Some(sample.pcm.as_slice()),
                        None => Some(&[][..]),
                    },
                    None => None,
                },
                None => None,
            };
            let gain = channels
                .get(usize::from(voice.channel))
                .copied()
                .unwrap_or_default()
                .gain();
            voice.render(out, pcm, gain);
        }
        voices.retain(|voice| !voice.is_finished());
    }

    /// Applies one message.
    fn apply(&mut self, event: MidiEvent) {
        match event {
            MidiEvent::NoteOn {
                channel,
                key,
                velocity,
            } => {
                if velocity == 0 {
                    self.note_off(channel, key);
                } else {
                    self.note_on(channel, key, velocity);
                }
            }
            MidiEvent::NoteOff { channel, key } => self.note_off(channel, key),
            MidiEvent::ProgramChange {
                channel,
                bank,
                program,
            } => {
                if let Some(slot) = self.channels.get_mut(usize::from(channel)) {
                    slot.bank = bank;
                    slot.program = program.min(127);
                }
            }
            MidiEvent::ControlChange {
                channel,
                control,
                value,
            } => {
                if let Some(slot) = self.channels.get_mut(usize::from(channel)) {
                    slot.control(control, value);
                }
            }
            // Bend is accepted and has no effect: this crate's own scores do
            // not bend, and a bend that moved a voice would have to reach into
            // every sounding one. Saying so is better than a field nothing
            // reads.
            MidiEvent::PitchBend { .. } => {}
            MidiEvent::AllNotesOff { channel } => {
                for voice in &mut self.voices {
                    if voice.channel == channel {
                        voice.release();
                    }
                }
            }
            MidiEvent::AllSoundOff { channel } => {
                for voice in &mut self.voices {
                    if voice.channel == channel {
                        voice.cut(0.0);
                    }
                }
            }
        }
    }

    /// Starts a note.
    fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        let state = self
            .channels
            .get(usize::from(channel))
            .copied()
            .unwrap_or_default();
        let amplitude = velocity_gain(velocity);
        let started = match self.bank.as_ref() {
            Some(bank) => {
                let Some(preset) = bank.preset(state.bank_for(channel), u16::from(state.program))
                else {
                    return;
                };
                gens::resolve(bank, preset, key, velocity)
                    .into_iter()
                    .map(|articulation| {
                        let rate = bank
                            .samples
                            .get(articulation.sample)
                            .map_or(self.sample_rate, |sample| sample.sample_rate);
                        Voice::sampled(
                            channel,
                            key,
                            articulation,
                            self.sample_rate,
                            rate,
                            amplitude,
                        )
                    })
                    .collect()
            }
            None => alloc::vec![Voice::oscillated(
                channel,
                key,
                state.waveform,
                self.sample_rate,
                amplitude,
                state.pan(),
            )],
        };
        for voice in started {
            if voice.exclusive != 0 {
                let group = voice.exclusive;
                for held in &mut self.voices {
                    if held.exclusive == group && held.key != key {
                        held.cut(STEAL);
                    }
                }
            }
            self.make_room();
            self.voices.push(voice);
        }
    }

    /// Releases a note.
    fn note_off(&mut self, channel: u8, key: u8) {
        for voice in &mut self.voices {
            if voice.channel == channel && voice.key == key && !voice.is_releasing() {
                voice.release();
            }
        }
    }

    /// Takes the oldest voice when the pool is full.
    fn make_room(&mut self) {
        if self.voices.len() < MAX_VOICES {
            return;
        }
        if let Some(voice) = self.voices.first_mut() {
            voice.cut(STEAL);
        }
    }
}
