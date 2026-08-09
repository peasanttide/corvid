//! Hex, in both directions, and the grouping a digest table is written in.

use alloc::string::String;
use alloc::vec::Vec;

/// A digest as a golden table writes them: sixteen hex digits in groups of four.
#[must_use]
pub fn grouped(digest: u64) -> String {
    let mut text = String::with_capacity(23);
    text.push_str("0x");
    for group in 0..4 {
        if group != 0 {
            text.push('_');
        }
        for nibbles in 0..4 {
            let shift = 60 - group * 16 - nibbles * 4;
            // The mask leaves one nibble, so the narrowing is exact and
            // `u8::try_from` cannot fail. Saying it this way rather than with a
            // cast keeps the workspace's cast lints meaningful here.
            let digit = u8::try_from((digest >> shift) & 0xf).unwrap_or(0);
            text.push(nibble(digit));
        }
    }
    text
}

/// Bytes as a golden table writes them: two lowercase hex digits each.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(nibble(byte >> 4));
        text.push(nibble(byte & 0x0f));
    }
    text
}

/// The inverse of [`hex`], so a recorded row can be decoded rather than only
/// compared.
///
/// Whitespace is ignored and means nothing, which is what lets a long row be
/// written in groups. [`None`] when what is left is not whole pairs of hex
/// digits -- a row that has lost a character is a mistake in the table rather
/// than a wire-format break, and the two should not look alike.
#[must_use]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let high = value(*pair.first()?)?;
            let low = value(*pair.get(1)?)?;
            Some((high << 4) | low)
        })
        .collect()
}

/// A string as a Rust raw literal that actually parses.
///
/// A raw string is terminated by a quote followed by as many hashes as opened
/// it, so the number of hashes has to exceed the longest run already inside the
/// text. JSON is exactly where this bites: a recorded row for a struct with a
/// `String` field holding `"#` closes a `r#"..."#` early, and the report a person
/// was told to paste does not compile.
pub(super) fn raw(text: &str) -> String {
    let mut longest = 0_usize;
    let mut run: Option<usize> = None;
    for character in text.chars() {
        run = match (run, character) {
            (Some(hashes), '#') => Some(hashes + 1),
            (_, '"') => Some(0),
            _ => None,
        };
        if let Some(hashes) = run {
            longest = longest.max(hashes + 1);
        }
    }

    let mut literal = String::with_capacity(text.len() + 2 * longest + 4);
    literal.push('r');
    for _ in 0..longest {
        literal.push('#');
    }
    literal.push('"');
    literal.push_str(text);
    literal.push('"');
    for _ in 0..longest {
        literal.push('#');
    }
    literal
}

/// One hex digit's character.
fn nibble(value: u8) -> char {
    if value < 10 {
        char::from(b'0' + value)
    } else {
        char::from(b'a' + value - 10)
    }
}

/// One hex digit's value, and [`None`] for anything that is not one.
const fn value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}
