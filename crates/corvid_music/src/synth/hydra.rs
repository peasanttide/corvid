//! The nine parallel arrays a `SoundFont`'s `pdta` chunk is made of.
//!
//! Each of presets, instruments and samples is a header array plus arrays that
//! bound one another: a preset header names the first *bag* that belongs to it,
//! and the next header's first bag is where it stops. Each array therefore ends
//! with a terminal sentinel record whose only job is to bound the last real one.
//! Everything in this module is that walk, and every step of it is bounds
//! checked.

use alloc::string::String;
use alloc::vec::Vec;

use crate::synth::bank::{
    Generator, GeneratorAmount, GeneratorKind, Instrument, Preset, Sample, SampleKind, Zone,
};
use crate::synth::sf2::{BankError, name};

/// A little-endian reader over one fixed-size record.
///
/// Records arrive from `chunks_exact`, so they are always the declared length
/// and every read below has data. Reading sequentially rather than by offset
/// lets each parser read like the specification's own field table, and keeps
/// every access total: a short record yields zeroes rather than panicking.
struct Record<'a>(&'a [u8]);

impl<'a> Record<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        let (head, tail) = self.0.split_at(count.min(self.0.len()));
        self.0 = tail;
        head
    }

    fn u8(&mut self) -> u8 {
        self.take(1).first().copied().unwrap_or(0)
    }

    fn i8(&mut self) -> i8 {
        i8::from_le_bytes([self.u8()])
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap_or([0; 2]))
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap_or([0; 4]))
    }

    fn name(&mut self) -> String {
        name(self.take(20))
    }

    fn pair(&mut self) -> [u8; 2] {
        self.take(2).try_into().unwrap_or([0; 2])
    }
}

/// Splits a chunk into fixed-size records and parses each.
fn records<T>(
    data: Option<&[u8]>,
    chunk: &'static str,
    size: usize,
    parse: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>, BankError> {
    let data = data.ok_or(BankError::Missing { chunk })?;
    if !data.len().is_multiple_of(size) {
        return Err(BankError::RecordSize {
            chunk,
            found: data.len(),
            size,
        });
    }
    Ok(data.chunks_exact(size).map(parse).collect())
}

/// A `phdr` record: a preset and where its bags start.
struct PresetHeader {
    label: String,
    program: u16,
    bank: u16,
    bag: u16,
}

/// An `inst` record: an instrument and where its bags start.
struct InstrumentHeader {
    label: String,
    bag: u16,
}

/// A `pbag`/`ibag` record: where a zone's generators start.
struct Bag {
    generator: u16,
}

/// A `pgen`/`igen` record: one generator.
struct GeneratorRecord {
    operator: u16,
    amount: [u8; 2],
}

/// An `shdr` record: a sample's place in the pool and how it is played.
struct SampleHeader {
    label: String,
    start: u32,
    end: u32,
    loop_start: u32,
    loop_end: u32,
    sample_rate: u32,
    original_key: u8,
    correction: i8,
    kind: u16,
}

/// The nine arrays, parsed.
pub(crate) struct Hydra {
    preset_headers: Vec<PresetHeader>,
    preset_bags: Vec<Bag>,
    preset_generators: Vec<GeneratorRecord>,
    instrument_headers: Vec<InstrumentHeader>,
    instrument_bags: Vec<Bag>,
    instrument_generators: Vec<GeneratorRecord>,
    sample_headers: Vec<SampleHeader>,
}

impl Hydra {
    /// Reads the nine arrays out of a parsed `pdta`.
    ///
    /// The modulator arrays are required to be present and are then discarded:
    /// this crate applies no modulators, and carrying a list nothing reads would
    /// be a promise it does not keep.
    pub(crate) fn read(chunks: &[([u8; 4], &[u8])]) -> Result<Self, BankError> {
        let get = |tag: &[u8; 4]| -> Option<&[u8]> {
            chunks
                .iter()
                .find(|(held, _)| held == tag)
                .map(|(_, data)| *data)
        };
        records(get(b"pmod"), "pmod", 10, |_| ())?;
        records(get(b"imod"), "imod", 10, |_| ())?;
        Ok(Self {
            preset_headers: records(get(b"phdr"), "phdr", 38, parse_preset)?,
            preset_bags: records(get(b"pbag"), "pbag", 4, parse_bag)?,
            preset_generators: records(get(b"pgen"), "pgen", 4, parse_generator)?,
            instrument_headers: records(get(b"inst"), "inst", 22, parse_instrument)?,
            instrument_bags: records(get(b"ibag"), "ibag", 4, parse_bag)?,
            instrument_generators: records(get(b"igen"), "igen", 4, parse_generator)?,
            sample_headers: records(get(b"shdr"), "shdr", 46, parse_sample)?,
        })
    }

    /// The presets, without the terminal sentinel.
    pub(crate) fn presets(&self) -> Result<Vec<Preset>, BankError> {
        let mut out = Vec::with_capacity(self.preset_headers.len().saturating_sub(1));
        for pair in self.preset_headers.windows(2) {
            let [header, next] = pair else { continue };
            out.push(Preset {
                name: header.label.clone(),
                program: header.program,
                bank: header.bank,
                zones: zones(
                    &self.preset_bags,
                    &self.preset_generators,
                    header.bag,
                    next.bag,
                    "pbag",
                )?,
            });
        }
        Ok(out)
    }

    /// The instruments, without the terminal sentinel.
    pub(crate) fn instruments(&self) -> Result<Vec<Instrument>, BankError> {
        let mut out = Vec::with_capacity(self.instrument_headers.len().saturating_sub(1));
        for pair in self.instrument_headers.windows(2) {
            let [header, next] = pair else { continue };
            out.push(Instrument {
                name: header.label.clone(),
                zones: zones(
                    &self.instrument_bags,
                    &self.instrument_generators,
                    header.bag,
                    next.bag,
                    "ibag",
                )?,
            });
        }
        Ok(out)
    }

    /// The samples, cut out of `pool` and with their loop points rebased.
    ///
    /// A sample whose declared span is not in the pool comes out empty rather
    /// than failing the whole bank: one bad sample header should cost one
    /// instrument, not a hundred.
    pub(crate) fn samples(&self, pool: &[u8]) -> Vec<Sample> {
        let Some((_sentinel, headers)) = self.sample_headers.split_last() else {
            return Vec::new();
        };
        headers
            .iter()
            .map(|header| {
                // A looping sample's loop may run past its declared end, so the
                // copy reaches to whichever is further.
                let start = usize::try_from(header.start).unwrap_or(0);
                let end = usize::try_from(header.end.max(header.loop_end)).unwrap_or(0);
                let pcm = pool
                    .get(start.saturating_mul(2)..end.saturating_mul(2))
                    .map(|bytes| {
                        bytes
                            .chunks_exact(2)
                            .map(|pair| i16::from_le_bytes(pair.try_into().unwrap_or([0; 2])))
                            .collect()
                    })
                    .unwrap_or_default();
                Sample {
                    name: header.label.clone(),
                    pcm,
                    sample_rate: header.sample_rate.max(1),
                    loop_start: header.loop_start.saturating_sub(header.start),
                    loop_end: header.loop_end.saturating_sub(header.start),
                    original_key: header.original_key,
                    correction: header.correction,
                    kind: sample_kind(header.kind),
                }
            })
            .collect()
    }
}

/// Builds the zones bounded by bag indices `from ..= to`.
fn zones(
    bags: &[Bag],
    generators: &[GeneratorRecord],
    from: u16,
    to: u16,
    chunk: &'static str,
) -> Result<Vec<Zone>, BankError> {
    let from = usize::from(from);
    let to = usize::from(to);
    let mut out = Vec::with_capacity(to.saturating_sub(from));
    for index in from..to {
        let (Some(bag), Some(next)) = (bags.get(index), bags.get(index + 1)) else {
            return Err(BankError::OutOfRange {
                chunk,
                into: "bags",
            });
        };
        let slice = generators
            .get(usize::from(bag.generator)..usize::from(next.generator))
            .ok_or(BankError::OutOfRange {
                chunk,
                into: "generators",
            })?;
        out.push(Zone {
            generators: slice.iter().map(build_generator).collect(),
        });
    }
    Ok(out)
}

/// Reads a generator's two bytes the way its operator says to.
fn build_generator(record: &GeneratorRecord) -> Generator {
    let kind = GeneratorKind(u8::try_from(record.operator).unwrap_or(GeneratorKind::COUNT));
    let [low, high] = record.amount;
    let amount = if kind == GeneratorKind::KEY_RANGE || kind == GeneratorKind::VELOCITY_RANGE {
        GeneratorAmount::Range { low, high }
    } else if kind == GeneratorKind::INSTRUMENT || kind == GeneratorKind::SAMPLE_ID {
        GeneratorAmount::Index(u16::from_le_bytes(record.amount))
    } else {
        GeneratorAmount::Signed(i16::from_le_bytes(record.amount))
    };
    Generator { kind, amount }
}

/// Reads an `sfSampleType` flag.
fn sample_kind(flags: u16) -> SampleKind {
    match flags {
        1 => SampleKind::Mono,
        2 => SampleKind::Right,
        4 => SampleKind::Left,
        8 => SampleKind::Linked,
        0x8001 | 0x8002 | 0x8004 | 0x8008 => SampleKind::Rom,
        other => SampleKind::Other(other),
    }
}

fn parse_preset(bytes: &[u8]) -> PresetHeader {
    let mut record = Record(bytes);
    PresetHeader {
        label: record.name(),
        program: record.u16(),
        bank: record.u16(),
        bag: record.u16(),
    }
}

fn parse_instrument(bytes: &[u8]) -> InstrumentHeader {
    let mut record = Record(bytes);
    InstrumentHeader {
        label: record.name(),
        bag: record.u16(),
    }
}

fn parse_bag(bytes: &[u8]) -> Bag {
    let mut record = Record(bytes);
    Bag {
        generator: record.u16(),
    }
}

fn parse_generator(bytes: &[u8]) -> GeneratorRecord {
    let mut record = Record(bytes);
    GeneratorRecord {
        operator: record.u16(),
        amount: record.pair(),
    }
}

fn parse_sample(bytes: &[u8]) -> SampleHeader {
    let mut record = Record(bytes);
    let label = record.name();
    let start = record.u32();
    let end = record.u32();
    let loop_start = record.u32();
    let loop_end = record.u32();
    let sample_rate = record.u32();
    let original_key = record.u8();
    let correction = record.i8();
    let _link = record.u16();
    SampleHeader {
        label,
        start,
        end,
        loop_start,
        loop_end,
        sample_rate,
        original_key,
        correction,
        kind: record.u16(),
    }
}
