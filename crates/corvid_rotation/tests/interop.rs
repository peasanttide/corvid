//! Layout guarantees, the wire format, and the optional integrations.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::float_cmp,
    reason = "comparisons are against exactly representable references"
)]
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    reason = "tests reach into raw bit patterns on purpose, and their f64 references are written as plain arithmetic so they stay independent of the implementation"
)]

mod common;

use core::mem::{align_of, size_of};

use corvid_rotation::{Basis, FineRotation, Rotation, Versor};

#[test]
fn the_sizes_are_what_the_docs_claim() {
    assert_eq!((size_of::<Rotation>(), align_of::<Rotation>()), (4, 4));
    assert_eq!(
        (size_of::<FineRotation>(), align_of::<FineRotation>()),
        (8, 8)
    );
    assert_eq!((size_of::<Versor>(), align_of::<Versor>()), (16, 4));
    assert_eq!((size_of::<Basis>(), align_of::<Basis>()), (36, 4));
}

#[cfg(feature = "serde")]
#[test]
fn packed_rotations_serialize_as_bare_integers() {
    // This is what makes corvid_transform's 16 B and 32 B figures mean
    // something over the wire, so assert the serialized form rather than only
    // that a round trip succeeds.
    let r = Rotation::from_bits(0xDEAD_BEEF);
    assert_eq!(serde_json::to_string(&r).unwrap(), "3735928559");
    assert_eq!(serde_json::from_str::<Rotation>("3735928559").unwrap(), r);

    let f = FineRotation::from_bits(0x0123_4567_89AB_CDEF);
    assert_eq!(serde_json::to_string(&f).unwrap(), "81985529216486895");
    assert_eq!(
        serde_json::from_str::<FineRotation>("81985529216486895")
            .unwrap()
            .to_bits(),
        f.to_bits()
    );

    // Four bytes and eight bytes on the wire, not a struct of named fields.
    assert_eq!(
        serde_json::to_string(&Rotation::IDENTITY)
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        Rotation::IDENTITY.to_bits()
    );
}

mod digest_interop {
    use core::hash::Hash as _;

    use corvid_fixed::I2F30;
    use corvid_hash::{Digest, Hasher, digest};

    use super::{Basis, FineRotation, Rotation, Versor};

    /// A packed rotation absorbs one word — its canonical bit pattern — and a
    /// `FineRotation`'s is already 64 bits wide, so the const form of its
    /// digest is one `absorb` and no widening.
    const IDENTITY: Digest = Hasher::new()
        .absorb(FineRotation::IDENTITY.to_bits())
        .digest();

    /// The digest of the four components as the array a versor holds them in.
    ///
    /// This is what a versor's digest has to equal: the components in the order
    /// the type stores them, with their signs, and nothing else. Nothing
    /// type-checks that order, so this is what holds it in place.
    fn absorbed(components: [I2F30; 4]) -> Digest {
        digest(&components)
    }

    /// One component of the working types' goldens, as the exact rational
    /// `numerator / denominator` at `I2F30`'s Q30 scale.
    const fn q30(numerator: i64, denominator: i64) -> I2F30 {
        I2F30::from_bits(((numerator << 30) / denominator) as i32)
    }

    /// The quaternion `(1, 2, 4, 10) / 11`, which is exactly unit because
    /// `1 + 4 + 16 + 100` is `11²` and so passes `from_xyzw`'s check.
    ///
    /// Every component differs from every other and none is zero, which is what
    /// makes a golden written against it able to fail. A golden built from
    /// `IDENTITY`, whose components are three zeros and a one, would still match
    /// an implementation that absorbed them backwards.
    const TURNED: [I2F30; 4] = [q30(1, 11), q30(2, 11), q30(4, 11), q30(10, 11)];

    /// A versor absorbs its four components in `x`, `y`, `z`, `w` order.
    fn turned_versor() -> Digest {
        absorbed(TURNED)
    }

    /// The quaternion `(1, -2, 4, -10) / 11`: `TURNED`'s magnitudes, over the
    /// same exact denominator, with two components negative and `w` below zero.
    ///
    /// `TURNED` alone is sign-blind. All four of its components are strictly
    /// positive, so a digest that absorbed each component's *magnitude* would
    /// match its golden, and so would one that folded the double cover first —
    /// negating all four when `w` is negative, the way `FineRotation`
    /// legitimately does. This one is wrong under both: the magnitudes send it
    /// to `TURNED_VERSOR`, and the fold sends it to `(-1, 2, -4, 10) / 11`. `w`
    /// is also the largest component in magnitude and it is negative, so a fold
    /// keyed on the largest component rather than on `w` moves it too.
    const TILTED: [I2F30; 4] = [q30(1, 11), q30(-2, 11), q30(4, 11), q30(-10, 11)];

    /// `TILTED`'s digest, absorbed in the same order and with the same signs.
    fn tilted_versor() -> Digest {
        absorbed(TILTED)
    }

    /// The quaternion `(-1, -2, -4, -10) / 11`: `TURNED` with every component
    /// negated.
    ///
    /// `TURNED` and `TILTED` between them witness `y` and `w` negative and
    /// nothing else — `x` and `z` are positive in both — so a digest that
    /// dropped the sign of `x`, or of `z`, or of both, matched every golden
    /// above. This one is negative in all four places, which closes that: under
    /// an absolute value on any subset of the components it lands somewhere in
    /// the family `(±1, ±2, ±4, ±10) / 11`, and the only member of that family
    /// with this golden is itself.
    ///
    /// It re-catches the two mutations `TILTED` was added for, by a different
    /// route: the magnitudes send it to `TURNED_VERSOR`, and so does a fold of
    /// the double cover, since `w` is negative here too.
    const MIRRORED: [I2F30; 4] = [q30(-1, 11), q30(-2, 11), q30(-4, 11), q30(-10, 11)];

    /// `MIRRORED`'s digest, absorbed in the same order and with the same signs.
    fn mirrored_versor() -> Digest {
        absorbed(MIRRORED)
    }

    /// A packed [`Rotation`] absorbs its raw pattern in one word, widened with
    /// zeros rather than sign-extended.
    ///
    /// `IDENTITY` is the pattern to write this against because its chart index
    /// is `3`, which sets bit 31 — the only bit that tells the two widenings
    /// apart. Every other rotation in a digest test here has that bit clear, so
    /// a sign-extending implementation would agree with all of them.
    fn packed_identity() -> Digest {
        let mut hasher = Hasher::new();
        Rotation::IDENTITY.to_bits().hash(&mut hasher);
        hasher.digest()
    }

    /// The rotation matrix `TURNED` denotes, over the same denominator of
    /// `11² = 121`, so every entry is an exact rational before the Q30
    /// truncation and the rows land well inside `from_rows`'s tolerance.
    ///
    /// The nine entries are pairwise different and the matrix is not symmetric,
    /// so this catches a transposed — column-major — absorption as well as a
    /// dropped or reordered entry.
    const SPUN: [[I2F30; 3]; 3] = [
        [q30(81, 121), q30(-76, 121), q30(48, 121)],
        [q30(84, 121), q30(87, 121), q30(-4, 121)],
        [q30(-32, 121), q30(36, 121), q30(111, 121)],
    ];

    /// A basis absorbs its three rows, each of three entries, in reading order.
    fn spun_basis() -> Digest {
        digest(&SPUN)
    }

    /// Packs four raw components, which is the only way to get a pattern the
    /// encoder would never produce.
    const fn pack(c: [i16; 4]) -> FineRotation {
        FineRotation::from_bits(
            (c[0] as u16 as u64)
                | ((c[1] as u16 as u64) << 16)
                | ((c[2] as u16 as u64) << 32)
                | ((c[3] as u16 as u64) << 48),
        )
    }

    #[test]
    fn const_and_runtime_evaluation_agree() {
        assert_eq!(IDENTITY, digest(&FineRotation::IDENTITY));
    }

    #[test]
    fn a_non_canonical_fine_rotation_digests_as_the_rotation_it_denotes() {
        // The double cover gives one rotation two bit patterns, and equality
        // and hashing both route through `canonicalize`. So does the digest, or
        // a rotation that arrived over a wire with the other sign would mark as
        // a rotation nobody is holding.
        let canonical = pack([1000, 0, 0, 32000]);
        let flipped = pack([-1000, 0, 0, -32000]);
        assert_eq!(canonical, flipped);
        assert_ne!(canonical.to_bits(), flipped.to_bits());
        assert_eq!(digest(&canonical), digest(&flipped));
    }

    #[test]
    fn a_rotation_digests_by_the_bits_it_compares_by() {
        // `Rotation`'s own `Eq` is on the raw pattern — canonicalizing it costs
        // a decode and a re-encode, which is why it is not folded in — so the
        // digest has to tell apart exactly the patterns `Eq` tells apart, and
        // that only shows on a pair canonicalization would merge. `0` and `1`
        // are such a pair: the one-past-the-end field pattern folds onto `-511`,
        // so both canonicalize to `0x0010_0401` while comparing unequal here.
        let raw = Rotation::from_bits(0);
        let neighbor = Rotation::from_bits(1);
        assert_eq!(raw.canonicalize(), neighbor.canonicalize());
        assert_ne!(raw, neighbor);
        assert_ne!(digest(&raw), digest(&neighbor));
    }

    #[test]
    fn a_rotation_absorbs_its_pattern_widened_with_zeros() {
        // The pattern is a `u32` and the sponge takes `u64`, so the digest has
        // to say which widening. Zero extension is the one that matches `Eq`,
        // and the difference only shows above `1 << 31` — where the two
        // patterns pinned below live and where nothing else in the suite goes.
        assert!(
            Rotation::IDENTITY.to_bits() >> 31 == 1,
            "the golden is only load-bearing while its top bit is set"
        );
        assert_eq!(packed_identity(), digest(&Rotation::IDENTITY));

        // A second such pattern, so the golden pins the rule rather than one
        // constant, and its top-bit-clear twin, so a digest that simply dropped
        // bit 31 does not slip through either.
        const HIGH: Rotation = Rotation::from_bits(0x8765_4321);
        let mut high = Hasher::new();
        0x8765_4321_u32.hash(&mut high);
        assert_eq!(high.digest(), digest(&HIGH));
        assert_ne!(digest(&HIGH), digest(&Rotation::from_bits(0x0765_4321)));
    }

    #[test]
    fn a_versor_absorbs_four_components_in_xyzw_order() {
        // A versor that dropped a component, or absorbed the four out of order,
        // would let two different orientations agree on a mark. The golden pins
        // the count and the order, which the inequalities below cannot: three of
        // `IDENTITY`'s four components are zero, so a first-component-only
        // implementation still separates it from anything else.
        let [x, y, z, w] = TURNED;
        let turned = Versor::from_xyzw(x, y, z, w).unwrap();
        assert_eq!(turned_versor(), digest(&turned));
        assert_ne!(digest(&Versor::IDENTITY), digest(&turned));
        assert_ne!(digest(&turned), Digest::ZERO);
    }

    #[test]
    fn a_versor_absorbs_the_signs_of_its_four_components() {
        // `TURNED`'s four components are all strictly positive, so the golden
        // above cannot see a sign at all. `TILTED` carries the same four
        // magnitudes with two of them negated, so an implementation that
        // absorbed `|component|` gives these two versors one mark.
        //
        // `TILTED` alone is not enough either. It negates `y` and `w`, and
        // `TURNED` is positive everywhere, so between them `x` and `z` are only
        // ever witnessed positive and an absolute value applied to just those
        // two matched both goldens. `MIRRORED` is negative in all four places,
        // so every component is now witnessed on both sides of zero and no
        // subset of the four can be sign-stripped without a golden moving.
        let mark = |components: [I2F30; 4]| {
            let [x, y, z, w] = components;
            digest(&Versor::from_xyzw(x, y, z, w).expect("the components are unit"))
        };
        assert_eq!(turned_versor(), mark(TURNED));
        assert_eq!(tilted_versor(), mark(TILTED));
        assert_eq!(mirrored_versor(), mark(MIRRORED));

        // Three sign patterns over one set of four magnitudes, so anything that
        // reaches the sponge through `|component|` collapses at least two of
        // these onto one mark.
        assert_ne!(mark(TURNED), mark(TILTED));
        assert_ne!(mark(TURNED), mark(MIRRORED));
        assert_ne!(mark(TILTED), mark(MIRRORED));

        // And the claim stated directly: across the three, every one of the four
        // slots is seen both above and below zero.
        for slot in 0..4 {
            assert!(
                [TURNED, TILTED, MIRRORED]
                    .iter()
                    .any(|c| c[slot].to_bits() > 0),
                "component {slot} is never witnessed positive"
            );
            assert!(
                [TURNED, TILTED, MIRRORED]
                    .iter()
                    .any(|c| c[slot].to_bits() < 0),
                "component {slot} is never witnessed negative"
            );
        }
    }

    #[test]
    fn a_versor_and_its_negation_digest_differently() {
        // `src/versor.rs` and the README both claim this, and until now nothing
        // tested it. It is the claim most at risk: `FineRotation` folds the
        // double cover on its way into the sponge, and folding it here too —
        // negating all four components when `w`, or the first component, or the
        // largest, is negative — is the obvious thing to copy across. Every such
        // fold makes one of the pairs below digest alike.
        //
        // The pairs differ in which component such a fold would key on:
        // `IDENTITY` is zero everywhere but `w`, `TURNED` is positive
        // everywhere, `TILTED` has its largest component negative while its
        // first is positive, and `TILTED` reversed has its first negative while
        // `w` is positive.
        for components in [
            Versor::IDENTITY.to_xyzw(),
            TURNED,
            TILTED,
            [TILTED[3], TILTED[2], TILTED[1], TILTED[0]],
        ] {
            let [x, y, z, w] = components;
            let q = Versor::from_xyzw(x, y, z, w).expect("the components are unit");
            let negated = q.negate();
            assert_ne!(q, negated, "{q:?}");
            assert_ne!(digest(&q), digest(&negated), "{q:?}");
            // Negating twice is the identity, so this really is one pair of
            // patterns and not a rounding artefact of `negate`.
            assert_eq!(q, negated.negate());
            assert_eq!(digest(&q), digest(&negated.negate()));
        }
    }

    #[test]
    fn a_basis_absorbs_nine_entries_row_major() {
        // Same argument one dimension up, plus one this shape admits and the
        // versor does not: a column-major absorption is a plausible
        // implementation, and it gives a rotation the mark its inverse should
        // have, since the inverse here is the transpose. Only the golden rules
        // that out — a transposed implementation moves `spun` and its transpose
        // alike, so comparing those two would not notice.
        let spun = Basis::from_rows(SPUN).unwrap();
        assert_eq!(spun_basis(), digest(&spun));
        assert_ne!(digest(&Basis::IDENTITY), digest(&spun));
        assert_ne!(digest(&Basis::IDENTITY), Digest::ZERO);
    }
}

#[cfg(feature = "bytemuck")]
#[test]
fn the_working_types_are_plain_old_data() {
    let m = Basis::IDENTITY;
    let bytes: &[u8] = bytemuck::bytes_of(&m);
    assert_eq!(bytes.len(), 36);
    assert_eq!(bytemuck::pod_read_unaligned::<Basis>(bytes), m);

    let q = Versor::IDENTITY;
    assert_eq!(bytemuck::bytes_of(&q).len(), 16);
    assert_eq!(bytemuck::bytes_of(&Rotation::IDENTITY).len(), 4);
    assert_eq!(bytemuck::bytes_of(&FineRotation::IDENTITY).len(), 8);
}

#[cfg(feature = "mint")]
#[test]
fn mint_round_trips_a_versor() {
    let mut rng = common::Rng::new(0x1717_1717);
    for _ in 0..1_000 {
        let q = common::random_versor(&mut rng);
        let m: mint::Quaternion<f64> = q.into();
        let back = Versor::from(m);
        assert!(
            q.angle_to(back).to_degrees() < 0.01,
            "{q:?} became {back:?}"
        );
    }
}

#[cfg(feature = "nalgebra")]
#[test]
fn nalgebra_agrees_on_the_matrix_and_the_quaternion() {
    let mut rng = common::Rng::new(0x4A19_4A19);
    for _ in 0..1_000 {
        let q = common::random_versor(&mut rng);
        let m: nalgebra::Matrix3<f64> = q.to_basis().into();
        let u: nalgebra::UnitQuaternion<f64> = q.into();

        // nalgebra builds the same matrix from the same rotation.
        let reference = u.to_rotation_matrix();
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (m[(i, j)] - reference[(i, j)]).abs() < 1e-6,
                    "entry ({i}, {j}): {} vs {}",
                    m[(i, j)],
                    reference[(i, j)]
                );
            }
        }

        assert!(q.angle_to(Versor::from(u)).to_degrees() < 0.01);
    }
}
