//! A hash whose output does not change unpredictably when one input bit
//! changes is not a hash.
//!
//! The statistic here is the strict avalanche criterion measured per cell:
//! for each of the 64 x 64 pairs of an input bit and an output bit, flipping
//! that input bit must flip that output bit about half the time. A summed
//! count over all 64 output bits cannot see a mixer that leaves individual bits
//! welded to the input, because the cells that never flip are averaged against
//! the cells that always do. A per-cell bound sees exactly that, and is what the
//! numbers below check.

use core::hash::Hasher as _;

use corvid_hash::Hasher;

/// One application of the mixer, reached through the public API.
///
/// A hasher built from a seed and digested immediately has absorbed nothing,
/// so its length is zero and `digest` computes `mix(seed)` and nothing else.
/// That is the only way to observe a single round of diffusion from outside the
/// crate, and it is the observation that matters: the chain applies the mixer
/// twice for even a one-word input, and two applications of a badly degraded
/// mixer still look uniform to any of these statistics.
const fn mixed(word: u64) -> u64 {
    Hasher::with_seed(word).digest().to_u64()
}

/// The whole chain for a single absorbed word: seed, absorb, inject the
/// length, digest.
// `const` because it can be: every step is a `const fn`, and a helper that
// compiles in a `const` context still runs perfectly well at run time, which is
// where these tests call it.
const fn hash_word(word: u64) -> u64 {
    Hasher::new().absorb(word).digest().to_u64()
}

/// Samples per cell. Each cell is a count of successes in this many Bernoulli
/// trials, so under a fair coin it has mean `SAMPLES / 2` and standard
/// deviation `sqrt(SAMPLES) / 2` — here 8192 and 64.
const SAMPLES: u64 = 16_384;

/// How far a cell may sit from half before the mixer is called broken: six
/// standard deviations, or 2.3% of the sample count.
///
/// Six rather than three because there are 4096 cells and the worst of 4096
/// fair coins strays about four standard deviations on its own; the real mixer
/// measures 3.8 at its worst. The degraded mixers this bound was checked
/// against mostly do not stray a little further but pin cells at 0 or at
/// `SAMPLES`: a mixer with its multiplies deleted, or given `3` for its odd
/// constant, or cut to one round, fails by the whole sample count.
///
/// Sweeping a uniform shift distance across the whole word says where the bound
/// actually sits. It is red at 16 and below — sixteen throughout strays 16.3,
/// which pins nothing but is still nowhere near six — green from 17 to 54, red
/// at 55, green again at 56, and red from 57 up. So what this catches is a
/// distance at or below a quarter of a word, or one within a hair of the word
/// width, and what it lets through is the whole middle: 48 measures 3.62, which
/// is where a fair coin sits. `tests/golden.rs` is what holds the middle.
///
/// The green band is wider than the mixer deserves. At a million samples per
/// cell 17 strays 23.7 and 53 strays 30.8, against 3.52 for the real mixer, so
/// those are real biases this sample count cannot resolve. Raising `SAMPLES` to
/// find them would cost thirty seconds an assertion and still would not
/// separate 29 from 31, which is the question the alternation actually poses.
const TOLERANCE: u64 = 384;

/// Counts, for one input bit, how often each output bit flipped with it.
fn cells_for(input_bit: u32, under: fn(u64) -> u64) -> [u64; 64] {
    let mut cells = [0u64; 64];
    for sample in 0..SAMPLES {
        // A golden-ratio walk rather than a counter, so the samples are spread
        // over the whole word rather than clustered in the low bits.
        let base = sample.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let mut delta = under(base) ^ under(base ^ (1 << input_bit));
        while delta != 0 {
            cells[delta.trailing_zeros() as usize] += 1;
            delta &= delta - 1;
        }
    }
    cells
}

/// Asserts the strict avalanche criterion over all 4096 cells of `under`.
fn assert_strict_avalanche(what: &str, under: fn(u64) -> u64) {
    let low = SAMPLES / 2 - TOLERANCE;
    let high = SAMPLES / 2 + TOLERANCE;
    for input_bit in 0..64 {
        for (output_bit, flips) in cells_for(input_bit, under).into_iter().enumerate() {
            assert!(
                (low..=high).contains(&flips),
                "{what}: input bit {input_bit} flipped output bit {output_bit} \
                 {flips} times out of {SAMPLES}, outside {low}..={high}"
            );
        }
    }
}

#[test]
fn the_mixer_flips_every_output_bit_about_half_the_time() {
    // The property the whole construction rests on. A mixer that has lost a
    // round, lost its multiplies, or been given a shift distance that carries
    // nothing fails this on individual cells by the whole sample count.
    assert_strict_avalanche("mixer", mixed);
}

#[test]
fn an_absorbed_bit_reaches_every_output_bit() {
    // The same criterion over absorb-and-finish, which is what a caller
    // actually runs. It says the plumbing between the mixer and the digest
    // loses nothing: a bit of the absorbed word reaches every bit of the mark.
    assert_strict_avalanche("chain", hash_word);
}

#[test]
fn multi_word_inputs_do_not_collide() {
    use std::collections::HashSet;

    // Single words cannot collide — the chain is a composition of bijections
    // in one word, so it is injective by construction and finding no
    // collisions among them proves nothing. Two words are where 128 bits of
    // input are squeezed into 64 of state and the map is genuinely
    // many-to-one, so this is the first place a collision could be found. A
    // mixer that folded pairs together — the identity does, since it makes the
    // chain absorb `w0 ^ w1` and forget which was which — collides here on the
    // first transposed pair.
    const SIDE: u64 = 320;
    const PAIRS: usize = 320 * 320;
    let mut seen = HashSet::with_capacity(PAIRS);
    for first in 0..SIDE {
        for second in 0..SIDE {
            let mut hasher = Hasher::new();
            hasher.write_u64(first);
            hasher.write_u64(second);
            assert!(
                seen.insert(hasher.digest()),
                "({first}, {second}) collided with an earlier pair"
            );
        }
    }
    assert_eq!(seen.len(), PAIRS);
}

#[test]
fn word_order_matters() {
    let mut a = Hasher::new();
    a.write_u64(1);
    a.write_u64(2);
    let mut b = Hasher::new();
    b.write_u64(2);
    b.write_u64(1);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn trailing_zero_words_are_not_free() {
    let mut a = Hasher::new();
    a.write_u64(7);
    let mut b = Hasher::new();
    b.write_u64(7);
    b.write_u64(0);
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn empty_is_not_zero() {
    assert_ne!(Hasher::new().digest().to_u64(), 0);
}

#[test]
fn a_short_write_differs_from_a_zero_extended_long_one() {
    // Both absorb the same zero-extended word; only the injected byte count
    // tells them apart, which is why `write` counts bytes and not words.
    let mut short = Hasher::new();
    short.write(&[1, 2, 3]);
    let mut long = Hasher::new();
    long.write(&[1, 2, 3, 0, 0, 0, 0, 0]);
    assert_ne!(short.digest(), long.digest());
}

#[test]
fn eight_bytes_and_the_word_they_spell_agree() {
    // `write` reads little-endian on every target, so the two spellings of one
    // word are one input.
    let mut bytes = Hasher::new();
    bytes.write(&0x0123_4567_89ab_cdefu64.to_le_bytes());
    let mut word = Hasher::new();
    word.write_u64(0x0123_4567_89ab_cdef);
    assert_eq!(bytes.digest(), word.digest());
}
