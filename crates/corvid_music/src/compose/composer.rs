//! The engine: twelve parameters in, one bar out, one bar at a time.

use alloc::vec::Vec;

use crate::compose::phrase::{Material, Phrase, Setting};
use crate::compose::tension::{MAX_DEFERRALS, Tension};
use crate::compose::{Bar, Chord, Mode, MotifId, MotifPool, Parameters, Role};
use crate::compose::{arrange, cost, melody, ornament, phrase, search, voicing};
use crate::rng::Rng;

/// How many bars of history voice leading is joined against.
const HISTORY: usize = 2;
/// How many motifs are held back from the next draw.
const RECENT: usize = 2;
/// How hard the search works by default.
const ITERATIONS: u32 = 96;

/// What a trigger asked for.
///
/// Two states rather than an `Option<Option<MotifId>>`, because "nothing is
/// armed" and "a new phrase is armed, on whatever motif the pool offers" are
/// different answers and reading them off two layers of `Option` is how one
/// gets written where the other was meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Armed {
    /// A new phrase on whatever the pool draws.
    Drawn,
    /// A new phrase on this motif.
    On(MotifId),
}

/// Composes a bar of music at a time.
///
/// Reactive at bar level and inside it. The twelve parameters are read fresh
/// for every bar, and the six that can act inside one -- density, ornament,
/// register, grit, syncopation, dissonance -- do; the six that need a boundary
/// wait for a phrase, and [`arm`](Self::arm) or [`interrupt`](Self::interrupt)
/// is how a boundary is made to arrive early.
///
/// # What a seed promises
///
/// The same seed, the same parameters and the same pool give the same bars. That
/// is what makes any claim about this crate testable, and it is why every random
/// decision the composer makes -- the metre, the motif, the variation, the
/// search -- is drawn from one generator that a seed sets.
///
/// ```
/// use corvid_music::{Composer, Event, Motif, MotifId, Parameters, Step};
///
/// let mut composer = Composer::new(20_260_808, Parameters::default());
/// composer.motifs_mut().insert(Motif::new(
///     MotifId(1),
///     vec![
///         Event::note(Step::new(0), 1.0),
///         Event::note(Step::new(2), 1.0),
///         Event::note(Step::new(4), 2.0),
///     ],
/// ));
///
/// let bar = composer.next_bar();
/// assert_eq!(bar.index, 0);
/// assert!(!bar.is_silent());
/// assert_eq!(bar.motif, Some(MotifId(1)));
/// ```
#[derive(Clone, Debug)]
pub struct Composer {
    seed: u64,
    rng: Rng,
    parameters: Parameters,
    pool: MotifPool,
    phrase: Option<Phrase>,
    history: Vec<Bar>,
    tension: Tension,
    armed: Option<Armed>,
    recent: Vec<MotifId>,
    previous_chord: Option<Chord>,
    left: arrange::Left,
    lead_octave: Option<i8>,
    bar_index: u32,
    iterations: u32,
}

impl Composer {
    /// The dissonance at or below which the composer promises a bar with no
    /// parallel fifths or octaves in it.
    ///
    /// Above it, parallels are what the parameter asked for: the rules that are
    /// a matter of taste are scaled by `1 - 0.7 * dissonance`, and the search
    /// stops being made to succeed. Two rules are never scaled at any value --
    /// the tune is never obscured, and no line is written outside its range.
    pub const STRICT_DISSONANCE: f32 = 0.5;

    /// How many times a cadence may be refused before it lands anyway.
    ///
    /// Eight. Long enough that a listener notices being held, short enough that
    /// a game whose tension never stops rising still gets a phrase that ends.
    pub const MAX_DEFERRALS: u8 = MAX_DEFERRALS;

    /// A composer seeded with `seed`, driven by `parameters`, quoting nothing.
    ///
    /// A pool with no motifs in it writes accompaniment and no tune, which is a
    /// legitimate thing to want and is also what a caller gets if they forget to
    /// load a pack. [`motifs_mut`](Self::motifs_mut) is how material arrives.
    #[must_use]
    pub fn new(seed: u64, parameters: Parameters) -> Self {
        Self {
            seed,
            rng: Rng::new(seed),
            parameters,
            pool: MotifPool::new(),
            phrase: None,
            history: Vec::new(),
            tension: Tension::default(),
            armed: None,
            recent: Vec::new(),
            previous_chord: None,
            left: arrange::Left::default(),
            lead_octave: None,
            bar_index: 0,
            iterations: ITERATIONS,
        }
    }

    /// Sets how hard the voicing search works, in candidate moves per bar.
    ///
    /// Zero skips the search: the constructive voicing is already correct, and
    /// what the search buys is spacing, doubling and leaps. The promise about
    /// parallels is kept either way, because that is enforced afterwards.
    #[must_use]
    pub const fn with_search(mut self, iterations: u32) -> Self {
        self.iterations = iterations;
        self
    }

    /// The seed this composer was built with.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The parameters as they stand.
    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    /// Replaces the parameters. Read fresh at every bar.
    pub const fn set_parameters(&mut self, parameters: Parameters) {
        self.parameters = parameters;
    }

    /// The motifs this composer may quote.
    #[must_use]
    pub const fn motifs(&self) -> &MotifPool {
        &self.pool
    }

    /// The motifs, to add to or to warm.
    pub const fn motifs_mut(&mut self) -> &mut MotifPool {
        &mut self.pool
    }

    /// Says how much tension there is, from `0.0` to `1.0`.
    ///
    /// Read once per bar. What it is made of is the caller's business; what it
    /// does here is decide whether a cadence is allowed to land. While the last
    /// four bars' tension has a positive slope the cadence is refused, the
    /// penultimate bar repeats under a deceptive chord, and the phrase does not
    /// end -- up to [`MAX_DEFERRALS`](Self::MAX_DEFERRALS).
    pub const fn set_tension(&mut self, tension: f32) {
        self.tension.set(tension);
    }

    /// How many times the current cadence has been refused.
    #[must_use]
    pub const fn deferrals(&self) -> u8 {
        self.tension.deferrals()
    }

    /// Which bar the next call to [`next_bar`](Self::next_bar) will write.
    #[must_use]
    pub const fn bar_index(&self) -> u32 {
        self.bar_index
    }

    /// Whether the tension of the last few bars is still rising.
    #[must_use]
    pub fn tension_rising(&self) -> bool {
        self.tension.rising()
    }

    /// Starts a new phrase at the next bar, on `motif` if one is named.
    ///
    /// The bar-level answer to a trigger: the form, the metre and the mode are
    /// allowed to change, but the bar already handed out is left alone.
    /// [`interrupt`](Self::interrupt) is the other answer, and it is not.
    pub fn arm(&mut self, motif: Option<MotifId>) {
        self.armed = Some(motif.map_or(Armed::Drawn, Armed::On));
    }

    /// Cuts the bar last written short at `beat` and starts a new phrase.
    ///
    /// This is the mid-bar answer. Notes that have already started ring out --
    /// they are left exactly as they were, ends and all -- and everything that
    /// had not started is dropped. The bar's [`beats`](Bar::beats) becomes
    /// `beat` and its [`elided`](Bar::elided) becomes true, so a dancer reading
    /// the bar cuts with the music instead of finishing a bar nobody is
    /// playing.
    ///
    /// Answers the shortened bar, or `None` when nothing has been written yet.
    /// The composer keeps the shortened bar as its own history, so the next
    /// bar's voice leading joins the music that was actually heard.
    ///
    /// ```
    /// use corvid_music::{Composer, Event, Motif, MotifId, Parameters, Step};
    ///
    /// let mut composer = Composer::new(4, Parameters::default());
    /// composer.motifs_mut().insert(Motif::new(
    ///     MotifId(1),
    ///     vec![Event::note(Step::new(0), 0.5), Event::note(Step::new(1), 0.5)],
    /// ));
    /// let whole = composer.next_bar();
    /// let cut = composer.interrupt(0.5).unwrap_or_else(|| whole.clone());
    ///
    /// assert!(cut.elided);
    /// assert_eq!(cut.beats, 0.5);
    /// assert!(cut.onsets() <= whole.onsets());
    /// ```
    pub fn interrupt(&mut self, beat: f32) -> Option<Bar> {
        let beat = beat.max(0.0);
        let bar = self.history.last_mut()?;
        for voice in &mut bar.voices {
            voice.notes.retain(|note| note.beat < beat - 1e-4);
        }
        bar.beats = beat.min(bar.beats);
        bar.elided = true;
        let cut = bar.clone();
        self.armed = Some(Armed::Drawn);
        Some(cut)
    }

    /// Writes the next bar.
    ///
    /// Pure in everything but the composer's own state: the same seed, the same
    /// parameters and the same pool give the same bar, which is the property
    /// every test in this crate rests on.
    pub fn next_bar(&mut self) -> Bar {
        let parameters = self.parameters.clamped();
        let mode = Mode::from_darkness(parameters.mode_dark);
        self.tension.remember();
        let mut phrase = self.phrase_for(parameters, mode);

        let position = self.bar_index.saturating_sub(phrase.start);
        let (cadence, forced) = self.tension.cadence_for(position, &mut phrase);

        let events = phrase.quote.take(&phrase.events, phrase.beats_per_bar);
        let lead_range = voicing::range(Role::Lead, 0, parameters.register);
        let (octave, mut tune) = melody::place(
            &events,
            phrase.tonic,
            mode,
            lead_range,
            parameters.register,
            self.lead_octave,
        );
        self.lead_octave = Some(octave);
        ornament::decorate(
            &mut tune,
            parameters.ornament,
            cadence.is_some(),
            (phrase.tonic, mode),
            parameters.chromaticism,
            &mut self.rng,
        );

        let setting = Setting {
            previous: self.previous_chord,
            forced,
            tonic: phrase.tonic,
            mode,
            beats_per_bar: phrase.beats_per_bar,
        };
        let chord = phrase::choose(&tune, &setting, parameters, &mut self.rng);
        let classes = voicing::pitch_classes(chord, phrase.tonic, mode);
        let ceiling = arrange::floor_of(&tune).map_or(lead_range.0, |low| low.saturating_sub(1));

        let voices = arrange::lines(
            parameters,
            phrase.beats_per_bar,
            &classes,
            ceiling,
            tune,
            &self.left,
        );
        let mut bar = Bar {
            index: self.bar_index,
            tempo: parameters.tempo,
            beats: phrase.beats_per_bar,
            beats_per_bar: phrase.beats_per_bar,
            tonic: phrase.tonic,
            mode,
            chord,
            cadence,
            motif: phrase.motif,
            variation: phrase.variation,
            elided: false,
            voices,
        };

        let weights = cost::Weights::of(parameters);
        let previous = self.history.last().cloned();
        search::anneal(
            &mut bar,
            previous.as_ref(),
            &classes,
            ceiling,
            &weights,
            &mut self.rng,
            self.iterations,
        );
        if parameters.dissonance <= Self::STRICT_DISSONANCE {
            search::enforce(&mut bar, previous.as_ref(), &classes, ceiling);
        }

        self.finish(&bar, &mut phrase, chord);
        self.phrase = Some(phrase);
        bar
    }

    /// The phrase this bar belongs to, beginning a new one when it must.
    fn phrase_for(&mut self, parameters: Parameters, mode: Mode) -> Phrase {
        let stale = self.armed.is_some()
            || self.phrase.as_ref().is_none_or(|phrase| {
                self.bar_index.saturating_sub(phrase.start) >= phrase.length
                    || phrase.mode != mode
                    || phrase.voices != parameters.voices
            });
        if !stale && let Some(phrase) = self.phrase.take() {
            return phrase;
        }
        let forced = match self.armed.take() {
            Some(Armed::On(motif)) => Some(motif),
            _ => None,
        };
        let material = Material {
            pool: &self.pool,
            recent: &self.recent,
            forced,
        };
        let phrase = Phrase::begin(
            self.phrase.as_ref(),
            parameters,
            mode,
            self.bar_index,
            &material,
            &mut self.rng,
        );
        self.tension.release();
        if let Some(id) = phrase.motif {
            self.recent.retain(|held| *held != id);
            self.recent.push(id);
            while self.recent.len() > RECENT {
                self.recent.remove(0);
            }
        }
        phrase
    }

    /// Records what this bar leaves behind for the next one.
    fn finish(&mut self, bar: &Bar, phrase: &mut Phrase, chord: Chord) {
        self.previous_chord = Some(chord);
        self.left = arrange::left_by(&bar.voices);
        self.history.push(bar.clone());
        while self.history.len() > HISTORY {
            self.history.remove(0);
        }
        if let Some(id) = phrase.motif
            && phrase.quote.laps > 0
            && let Some(source) = self.pool.get(id)
        {
            let source = source.clone();
            phrase.vary(&source, &mut self.rng);
        }
        self.pool.cool(0.9);
        self.bar_index = self.bar_index.saturating_add(1);
    }
}
