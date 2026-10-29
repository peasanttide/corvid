//! The sound card: a [`Player`] on a thread of its own, a little ahead of a
//! `cpal` stream.
//!
//! # Why a thread between the synthesizer and the callback
//!
//! The audio callback runs against a deadline of a few milliseconds and may
//! not allocate, wait or panic -- `corvid_audio` sets the rule out at length.
//! The synthesizer allocates: a note starting is a voice pushed onto a list.
//! So the synthesizer does not run in the callback. A worker thread renders a
//! tenth of a second ahead into a queue whose capacity is reserved before the
//! stream starts, and the callback only pops from it, taking the lock without
//! waiting and writing silence for the one period it could not.
//!
//! The cost is latency: a song asked for is heard up to that tenth of a second
//! later, which for a soundtrack is nothing. The gain is applied in the
//! callback, so turning the music down is heard at once.
//!
//! The stream is built on, and lives on, the worker thread, which is what makes
//! [`Music`] itself free to cross threads -- unlike `corvid_audio::Audio`,
//! which holds its stream and so has to stay where it was opened.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use crate::{Bank, Error, Player, Song, Unavailable};

/// How far ahead of the device the worker renders, in seconds' tenths.
const AHEAD: u32 = 10;

/// Frames the worker renders at a time.
const BLOCK: usize = 1024;

/// How long the worker sleeps when it is far enough ahead.
const NAP: Duration = Duration::from_millis(5);

/// What the game asks of the worker.
#[derive(Debug)]
enum Command {
    /// Start this song from its beginning.
    Play(Song),
    /// Stop the song.
    Stop,
}

/// What the game's side and the worker both read.
#[derive(Debug, Default)]
struct Shared {
    /// The gain, as an `f32`'s bits, applied in the callback.
    gain: AtomicU32,
    /// Whether the worker's player has a song, as of its last look.
    playing: AtomicBool,
    /// Whether the last thing asked was a song rather than a stop.
    wants: AtomicBool,
    /// How many commands have been sent.
    sent: AtomicU64,
    /// How many the worker has carried out.
    done: AtomicU64,
}

/// Interleaved stereo, rendered and not yet played.
type Queue = Arc<Mutex<VecDeque<f32>>>;

/// A soundtrack playing to the machine's default output device.
///
/// Opening one starts a thread that opens the device, decodes the bank and
/// then plays whatever it is asked to. Dropping it stops the thread and closes
/// the stream.
///
/// ```no_run
/// # #[cfg(feature = "unison")]
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use corvid_music::{Bank, Music, Song};
///
/// // A machine with no sound card answers `Err`: carry on without music.
/// let music = Music::open(Bank::unison)?;
/// music.set_gain(0.5);
/// music.play(Song::parse(&std::fs::read("song.mid")?)?);
/// # Ok(()) }
/// # #[cfg(not(feature = "unison"))]
/// # fn main() {}
/// ```
#[derive(Debug)]
pub struct Music {
    /// Where commands go to the worker.
    commands: Sender<Command>,
    /// What both sides read.
    shared: Arc<Shared>,
    /// The device's rate.
    rate: u32,
}

impl Music {
    /// Opens the default output device and starts a worker that decodes the
    /// bank `load` returns and then plays whatever it is asked.
    ///
    /// The bank is decoded on the worker, after the device answered, so a
    /// machine without one never pays for the decoding and a game's frame is
    /// never held up by it. Until it is decoded the device plays silence, and
    /// a song asked for meanwhile starts when it is ready. A bank that will not
    /// decode is reported through `tracing` and leaves the music silent.
    ///
    /// # Errors
    ///
    /// [`Unavailable`], every case of which is the machine.
    pub fn open<F>(load: F) -> Result<Self, Unavailable>
    where
        F: FnOnce() -> Result<Bank, Error> + Send + 'static,
    {
        let (commands, inbox) = mpsc::channel();
        let (answer, opened) = mpsc::channel();
        let shared = Arc::new(Shared {
            gain: AtomicU32::new(1.0f32.to_bits()),
            ..Shared::default()
        });
        let theirs = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("corvid_music".into())
            .spawn(move || work(load, &inbox, &answer, &theirs))
            .map_err(|_| Unavailable::NoThread)?;
        let rate = opened.recv().map_err(|_| Unavailable::NoThread)??;
        Ok(Self {
            commands,
            shared,
            rate,
        })
    }

    /// The device's rate, in hertz.
    #[must_use]
    pub const fn rate(&self) -> u32 {
        self.rate
    }

    /// Starts `song` from its beginning, in place of whatever was playing.
    pub fn play(&self, song: Song) {
        self.ask(Command::Play(song), true);
    }

    /// Stops the song; its last notes release rather than click.
    pub fn stop(&self) {
        self.ask(Command::Stop, false);
    }

    /// How loud, from 0 (silent) to 1 (as the bank plays it), heard at once.
    pub fn set_gain(&self, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// Whether a song is playing, or asked for and about to.
    ///
    /// False once a song has ended and its tail rung out, which is a game's cue
    /// to choose the next one.
    #[must_use]
    pub fn playing(&self) -> bool {
        if self.shared.sent.load(Ordering::SeqCst) == self.shared.done.load(Ordering::SeqCst) {
            self.shared.playing.load(Ordering::SeqCst)
        } else {
            self.shared.wants.load(Ordering::SeqCst)
        }
    }

    /// Hands the worker a command, noting what it will make of `playing`.
    fn ask(&self, command: Command, wants: bool) {
        self.shared.wants.store(wants, Ordering::SeqCst);
        self.shared.sent.fetch_add(1, Ordering::SeqCst);
        // A worker that has gone has gone because the device did, and it said
        // so then; the music is silent either way.
        let _ = self.commands.send(command);
    }
}

/// The worker: open the device, answer, decode the bank, then keep the queue
/// a tenth of a second full until the game's side is dropped.
fn work<F>(
    load: F,
    inbox: &Receiver<Command>,
    answer: &Sender<Result<u32, Unavailable>>,
    shared: &Arc<Shared>,
) where
    F: FnOnce() -> Result<Bank, Error>,
{
    let (stream, rate, queue) = match open(shared) {
        Ok(opened) => opened,
        Err(why) => {
            let _ = answer.send(Err(why));
            return;
        }
    };
    let _ = answer.send(Ok(rate));
    tracing::info!(name: "corvid_music.opened", rate, "the music has a device");
    let mut player = match load() {
        Ok(bank) => Player::new(&bank, rate),
        Err(why) => {
            tracing::warn!(name: "corvid_music.no_bank", why = %why, "the music's bank would not decode, so the music is silent");
            return;
        }
    };
    let ahead = 2 * usize::try_from(rate / AHEAD).unwrap_or(BLOCK);
    let mut block = vec![0.0f32; 2 * BLOCK];
    loop {
        loop {
            let command = match inbox.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    drop(stream);
                    return;
                }
            };
            let done = match command {
                Command::Play(song) => player.play(song),
                Command::Stop => player.stop(),
            };
            if let Err(why) = done {
                tracing::warn!(name: "corvid_music.refused", why = %why, "the synthesizer refused a command");
            }
            shared.playing.store(player.playing(), Ordering::SeqCst);
            shared.done.fetch_add(1, Ordering::SeqCst);
        }
        let queued = queue.lock().map_or(ahead, |queue| queue.len());
        if queued >= ahead {
            std::thread::sleep(NAP);
            continue;
        }
        if let Err(why) = player.fill(&mut block) {
            tracing::warn!(name: "corvid_music.refused", why = %why, "the synthesizer refused a render");
            block.fill(0.0);
        }
        shared.playing.store(player.playing(), Ordering::SeqCst);
        if let Ok(mut queue) = queue.lock() {
            queue.extend(block.iter().copied());
        }
    }
}

/// Opens the default output device with a stream reading `shared`'s gain and
/// a queue sized before it starts.
fn open(shared: &Arc<Shared>) -> Result<(cpal::Stream, u32, Queue), Unavailable> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Unavailable::NoDevice)?;
    let supported = device
        .default_output_config()
        .map_err(Unavailable::NoConfig)?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate;
    let capacity = 2 * usize::try_from(rate / AHEAD).unwrap_or(BLOCK) + 4 * BLOCK;
    let queue: Queue = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
    let stream = build(&device, &config, format, &queue, shared)?;
    stream.play().map_err(Unavailable::Unstarted)?;
    Ok((stream, rate, queue))
}

/// Builds the stream in whichever sample format the device asked for.
fn build(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    queue: &Queue,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, Unavailable> {
    /// What a broken stream is reported as; the music goes silent.
    fn complained(why: &cpal::Error) {
        tracing::warn!(name: "corvid_music.stream", why = %why, "the music's device reported an error");
    }

    /// The stream for one concrete sample type.
    macro_rules! build {
        ($sample:ty) => {{
            let queue = Arc::clone(queue);
            let shared = Arc::clone(shared);
            let channels = usize::from(config.channels).max(1);
            device.build_output_stream(
                config.clone(),
                move |out: &mut [$sample], _: &cpal::OutputCallbackInfo| {
                    fill(&queue, &shared, out, channels);
                },
                |why| complained(&why),
                None,
            )
        }};
    }

    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U8 => build!(u8),
        other => return Err(Unavailable::Unwritable(other)),
    };
    stream.map_err(Unavailable::Unopened)
}

/// One callback: pop what the worker rendered, scaled by the gain, into the
/// device's channels -- left and right to the first two, their mean to a mono
/// device and to any past the second. Silence for whatever is not there.
fn fill<S: cpal::Sample + cpal::FromSample<f32>>(
    queue: &Queue,
    shared: &Shared,
    out: &mut [S],
    channels: usize,
) {
    let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
    let mut waiting = queue.try_lock().ok();
    for slot in out.chunks_mut(channels) {
        let (left, right) = waiting
            .as_mut()
            .and_then(|queue| Some((queue.pop_front()?, queue.pop_front()?)))
            .unwrap_or((0.0, 0.0));
        let (left, right) = (left * gain, right * gain);
        let mono = 0.5 * (left + right);
        let both = slot.len() >= 2;
        for (at, sample) in slot.iter_mut().enumerate() {
            let value = match at {
                0 if both => left,
                1 => right,
                _ => mono,
            };
            *sample = S::from_sample(value);
        }
    }
}
