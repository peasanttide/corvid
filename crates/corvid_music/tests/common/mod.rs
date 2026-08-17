//! A `.sf2` image, built byte by byte, so the parser is tested against the
//! format rather than against a file somebody happened to have.
//!
//! The bank it builds is the smallest one that is still a bank: one preset,
//! one instrument, one looping sample covering the whole keyboard. Every array
//! carries the terminal sentinel record the specification requires, because a
//! parser that worked without them would be reading a format nobody writes.

#![cfg(feature = "synth")]

/// How many frames the sample holds.
pub(crate) const FRAMES: usize = 256;
/// The key the sample was recorded at.
pub(crate) const ROOT_KEY: u8 = 60;
/// The rate it was recorded at.
pub(crate) const SAMPLE_RATE: u32 = 22_050;
/// The name the bank gives itself.
pub(crate) const BANK_NAME: &str = "Test Bank";
/// The name the one preset gives itself.
pub(crate) const PRESET_NAME: &str = "Reed";

/// One RIFF chunk: a tag, a little-endian length, the data, and a pad byte when
/// the data is an odd length.
fn chunk(tag: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 9);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// A `LIST` chunk of the named type.
fn list(kind: [u8; 4], children: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    for child in children {
        body.extend_from_slice(child);
    }
    chunk(*b"LIST", &body)
}

/// A fixed-width, NUL-padded name field.
fn name(text: &str, width: usize) -> Vec<u8> {
    let mut out = text.as_bytes().to_vec();
    out.truncate(width.saturating_sub(1));
    out.resize(width, 0);
    out
}

/// One `phdr` record.
fn preset_header(label: &str, program: u16, bank: u16, bag: u16) -> Vec<u8> {
    let mut out = name(label, 20);
    out.extend_from_slice(&program.to_le_bytes());
    out.extend_from_slice(&bank.to_le_bytes());
    out.extend_from_slice(&bag.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// One `inst` record.
fn instrument_header(label: &str, bag: u16) -> Vec<u8> {
    let mut out = name(label, 20);
    out.extend_from_slice(&bag.to_le_bytes());
    out
}

/// One `pbag`/`ibag` record.
fn bag(generator: u16, modulator: u16) -> Vec<u8> {
    let mut out = generator.to_le_bytes().to_vec();
    out.extend_from_slice(&modulator.to_le_bytes());
    out
}

/// One `pgen`/`igen` record.
fn generator(operator: u16, amount: [u8; 2]) -> Vec<u8> {
    let mut out = operator.to_le_bytes().to_vec();
    out.extend_from_slice(&amount);
    out
}

/// One `pmod`/`imod` record. Only the terminal one is ever written here.
fn modulator() -> Vec<u8> {
    vec![0; 10]
}

/// One `shdr` record.
#[expect(
    clippy::too_many_arguments,
    reason = "the specification's own field list, written out so that a reader \
              can check it against the table"
)]
fn sample_header(
    label: &str,
    start: u32,
    end: u32,
    loop_start: u32,
    loop_end: u32,
    rate: u32,
    key: u8,
    kind: u16,
) -> Vec<u8> {
    let mut out = name(label, 20);
    for value in [start, end, loop_start, loop_end, rate] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.push(key);
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out
}

/// Joins a list of records into one chunk body.
fn joined(records: &[Vec<u8>]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|record| record.iter().copied())
        .collect()
}

/// The sample audio: one cycle of a sawtooth per eight frames, loud enough that
/// a render of it is unmistakably not silence.
///
/// Written as integer arithmetic so that the fixture is the same bytes on every
/// machine: a golden that was rounded into existence is a golden that can
/// change under a compiler.
fn pcm() -> Vec<i16> {
    (0..FRAMES)
        .map(|index| {
            let step = i16::try_from(index % 8).unwrap_or(0);
            step * 4_000 - 16_000
        })
        .collect()
}

/// A complete, well-formed `.sf2` image.
#[must_use]
pub(crate) fn image() -> Vec<u8> {
    let pool: Vec<u8> = pcm().into_iter().flat_map(i16::to_le_bytes).collect();

    let info = list(
        *b"INFO",
        &[
            chunk(*b"ifil", &[2, 0, 1, 0]),
            chunk(*b"isng", &name("EMU8000", 8)),
            chunk(*b"INAM", &name(BANK_NAME, 16)),
        ],
    );
    let sdta = list(*b"sdta", &[chunk(*b"smpl", &pool)]);

    let frames = u32::try_from(FRAMES).unwrap_or(0);
    let pdta = list(
        *b"pdta",
        &[
            chunk(
                *b"phdr",
                &joined(&[
                    preset_header(PRESET_NAME, 0, 0, 0),
                    preset_header("EOP", 0, 0, 1),
                ]),
            ),
            chunk(*b"pbag", &joined(&[bag(0, 0), bag(1, 0)])),
            chunk(*b"pmod", &modulator()),
            // The preset zone points at instrument zero; the second record is
            // the terminal generator the last bag is bounded by.
            chunk(
                *b"pgen",
                &joined(&[generator(41, 0u16.to_le_bytes()), generator(0, [0, 0])]),
            ),
            chunk(
                *b"inst",
                &joined(&[instrument_header("Reed", 0), instrument_header("EOI", 1)]),
            ),
            chunk(*b"ibag", &joined(&[bag(0, 0), bag(3, 0)])),
            chunk(*b"imod", &modulator()),
            // Key range 0..=127, sample mode 1 (loop), sample zero, terminal.
            chunk(
                *b"igen",
                &joined(&[
                    generator(43, [0, 127]),
                    generator(54, 1i16.to_le_bytes()),
                    generator(53, 0u16.to_le_bytes()),
                    generator(0, [0, 0]),
                ]),
            ),
            chunk(
                *b"shdr",
                &joined(&[
                    sample_header("Reed", 0, frames, 8, frames - 8, SAMPLE_RATE, ROOT_KEY, 1),
                    sample_header("EOS", 0, 0, 0, 0, 1, 0, 1),
                ]),
            ),
        ],
    );

    let mut body = b"sfbk".to_vec();
    body.extend_from_slice(&info);
    body.extend_from_slice(&sdta);
    body.extend_from_slice(&pdta);
    chunk(*b"RIFF", &body)
}
