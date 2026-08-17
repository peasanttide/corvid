//! Eighty-nine stars, embedded, so that a sky needs no download.
//!
//! Every row is transcribed from SIMBAD's record for the star, whose astrometry
//! is van Leeuwen's re-reduction of the Hipparcos data, *Astronomy and
//! Astrophysics* 474, 653 (2007), on the ICRS at epoch and equinox J2000.0, or
//! Gaia DR3 where SIMBAD prefers it. The photometry is the same record's
//! Johnson `V` and `B`, and the colour index is their difference. A star whose
//! record was missing any of the ten columns was left out rather than filled
//! in, which is why four naked-eye stars that never rise north of the tropics
//! -- Acrux, Hadar, Algieba and Alsephina -- are not here.
//!
//! This is a bright-star set and not a catalogue: the naked-eye sky down to
//! about magnitude 2.5, plus eight nearby high-proper-motion stars that make
//! the rollback measurable rather than theoretical. Barnard's Star, Kapteyn's
//! Star, Groombridge 1830 and their neighbours are the rows that move far
//! enough over two centuries to catch an error in [`Star::propagated`], and
//! `tests/stars.rs` uses them for exactly that.
//!
//! A game wanting the several thousand stars a dark-adapted eye and an
//! engraving want does not embed them in a crate; it bakes them into an asset
//! with a record naming the catalogue and its version, the same as any scan.
//! What is here is enough to test the arithmetic and enough to draw a
//! recognisable sky.

use crate::star::Star;

/// The table, as a table.
///
/// Ten columns and no field names, because a catalogue with the column headings
/// repeated on every row is a catalogue nobody can read down. The order is the
/// order of [`Star`]'s own fields, and the header comment below is the only
/// place it is written.
macro_rules! catalogue {
    ($($name:literal $hip:literal $ra:literal $dec:literal $pm_ra:literal
       $pm_dec:literal $parallax:literal $radial:literal $magnitude:literal
       $colour:literal;)*) => {
        [$(Star {
            name: $name,
            hip: $hip,
            right_ascension: $ra,
            declination: $dec,
            proper_motion_ra: $pm_ra,
            proper_motion_dec: $pm_dec,
            parallax: $parallax,
            radial_velocity: $radial,
            magnitude: $magnitude,
            colour_index: $colour,
        }),*]
    };
}

/// The embedded star set, in order of increasing `V` magnitude.
///
/// ```
/// use corvid_sky::BRIGHT_STARS;
///
/// // Sorted, which is what makes `corvid_sky::brighter_than` a prefix rather
/// // than a filter.
/// assert!(BRIGHT_STARS.windows(2).all(|two| two[0].magnitude <= two[1].magnitude));
/// assert_eq!(BRIGHT_STARS[0].name, "Sirius");
/// ```
pub const BRIGHT_STARS: [Star; 89] = catalogue![
    //  name                  HIP     right ascension  declination     pm RA      pm dec   parallax    v_rad     V     B-V
    "Sirius" 32_349 101.287_155_33 -16.716_115_86 -546.01 -1_223.07 379.21 -5.5 -1.46 0.0;
    "Canopus" 30_438 95.987_957_83 -52.695_661_38 19.93 23.24 10.55 20.3 -0.74 0.15;
    "Arcturus" 69_673 213.915_300_29 19.182_409_16 -1_093.39 -2_000.06 88.83 -5.229 -0.05 1.23;
    "Rigil Kentaurus" 71_683 219.902_058_33 -60.833_992_69 -3_679.25 473.67 742.12 -15.252 0.01 0.71;
    "Vega" 91_262 279.234_734_79 38.783_688_96 200.94 286.23 130.23 -13.5 0.03 0.0;
    "Capella" 24_608 79.172_327_94 45.997_991_47 75.25 -426.89 76.2 29.19 0.08 0.8;
    "Rigel" 24_436 78.634_467_07 -8.201_638_36 1.31 0.5 3.78 17.8 0.13 -0.03;
    "Procyon" 37_279 114.825_497_91 5.224_987_56 -714.59 -1_036.8 284.56 -4.505 0.37 0.42;
    "Betelgeuse" 27_989 88.792_938_99 7.407_064 27.54 11.3 6.55 21.91 0.42 1.85;
    "Achernar" 7_588 24.428_522_83 -57.236_752_81 87.0 -38.24 23.39 8.47 0.46 -0.16;
    "Altair" 97_649 297.695_827_3 8.868_321_2 536.23 385.29 194.95 -26.6 0.76 0.22;
    "Aldebaran" 21_421 68.980_162_79 16.509_302_35 63.45 -188.94 48.94 54.398 0.86 1.54;
    "Antares" 80_763 247.351_915_42 -26.432_002_61 -12.11 -23.3 5.89 -3.5 0.91 1.84;
    "Spica" 65_474 201.298_247_36 -11.161_319_49 -42.35 -30.67 13.06 -3.31 0.97 -0.23;
    "Pollux" 37_826 116.328_957_77 28.026_198_89 -626.55 -45.8 96.54 3.391 1.14 1.0;
    "Fomalhaut" 113_368 344.412_692_72 -29.622_237_03 328.95 -164.67 129.81 6.5 1.16 0.09;
    "Deneb" 102_098 310.357_979_75 45.280_338_81 2.01 1.85 2.31 -4.9 1.25 0.09;
    "Mimosa" 62_434 191.930_286_56 -59.688_772 -42.97 -16.18 11.71 10.3 1.25 -0.23;
    "Toliman" 71_681 219.896_096_29 -60.837_527_57 -3_614.39 802.98 742.12 -22.586 1.33 0.88;
    "Regulus" 49_669 152.092_962_44 11.967_208_78 -248.73 5.59 41.13 0.72 1.4 -0.16;
    "Adhara" 33_579 104.656_453_15 -28.972_086_16 3.24 1.33 8.05 27.3 1.5 -0.21;
    "Castor" 36_850 113.649_471_64 31.888_282_22 -191.45 -145.19 64.12 -11.23 1.58 0.04;
    "Shaula" 85_927 263.402_167_18 -37.103_823_55 -8.53 -30.8 5.71 -3.0 1.63 -0.14;
    "Gacrux" 61_084 187.791_498_38 -57.113_213_46 28.23 -265.08 36.83 21.0 1.64 1.59;
    "Bellatrix" 25_336 81.282_763_56 6.349_703_26 -8.11 -12.88 12.92 17.31 1.64 -0.22;
    "Elnath" 25_428 81.572_971_33 28.607_451_72 22.76 -173.58 24.36 9.2 1.65 -0.13;
    "Miaplacidus" 45_238 138.299_906_08 -69.717_207_6 -156.47 108.95 28.82 -5.1 1.69 0.0;
    "Alnilam" 26_311 84.053_388_94 -1.201_919_14 1.44 -0.78 1.65 27.3 1.69 -0.18;
    "Alnair" 109_268 332.058_269_7 -46.960_974_38 126.69 -147.47 32.29 10.9 1.71 -0.13;
    "Alnitak" 26_727 85.189_694_43 -1.942_573_59 3.19 2.03 4.43 18.5 1.77 -0.21;
    "Alioth" 62_956 193.507_289_97 55.959_822_96 111.91 -8.24 39.51 -12.7 1.77 -0.02;
    "Dubhe" 54_061 165.931_964_67 61.751_034_69 -134.11 -34.7 26.54 -9.4 1.79 1.07;
    "Mirfak" 15_863 51.080_708_72 49.861_179_29 23.75 -26.23 6.44 -2.158 1.79 0.48;
    "Kaus Australis" 90_185 276.042_993_35 -34.384_616_49 -39.42 -124.2 22.76 -15.0 1.81 0.01;
    "Wezen" 34_444 107.097_850_21 -26.393_199_58 -3.12 3.31 2.03 33.67 1.84 0.68;
    "Sargas" 86_228 264.329_707_72 -42.997_827_99 5.54 -3.12 10.86 5.18 1.85 0.44;
    "Avior" 41_037 125.628_480_24 -59.509_484_19 -25.52 22.06 5.39 11.6 1.86 1.27;
    "Alkaid" 67_301 206.885_157_34 49.313_266_73 -121.17 -14.91 31.38 -13.4 1.86 -0.19;
    "Atria" 82_273 252.166_229_51 -69.027_711_85 17.99 -31.58 8.35 -3.0 1.88 1.45;
    "Menkalinan" 28_360 89.882_178_87 44.947_432_57 -56.44 -0.95 40.21 -15.75 1.9 0.03;
    "Peacock" 100_751 306.411_904_37 -56.735_089_73 6.9 -86.02 18.24 2.0 1.918 -0.127;
    "Alhena" 31_681 99.427_960_43 16.399_280_43 13.81 -54.96 29.84 -12.63 1.92 0.0;
    "Mirzam" 30_324 95.674_938_97 -17.955_918_71 -3.23 -0.78 6.62 33.7 1.97 -0.24;
    "Alphard" 46_390 141.896_844_6 -8.658_599_53 -15.23 34.37 18.09 -4.561 1.97 1.45;
    "Hamal" 9_884 31.793_357_1 23.462_417_55 188.55 -148.08 49.56 -14.412 2.01 1.16;
    "Diphda" 3_419 10.897_378_74 -17.986_606_32 232.55 31.99 33.86 13.257 2.01 1.01;
    "Polaris" 11_767 37.954_560_67 89.264_108_97 44.48 -11.85 7.54 -16.42 2.02 0.6;
    "Menkent" 68_933 211.670_614_68 -36.369_954_74 -520.53 -518.06 55.45 1.3 2.05 0.99;
    "Mirach" 5_447 17.433_016_17 35.620_557_65 175.9 -112.2 16.52 0.609 2.05 1.57;
    "Alpheratz" 677 2.096_916_19 29.090_431_12 137.46 -163.44 33.62 -10.1 2.06 -0.11;
    "Saiph" 27_366 86.939_120_17 -9.669_604_92 1.46 -1.28 5.04 20.5 2.06 -0.18;
    "Nunki" 92_855 283.816_360_41 -26.296_724_11 15.14 -53.43 14.32 -11.2 2.067 -0.144;
    "Rasalhague" 86_032 263.733_622_72 12.560_037_39 108.07 -221.57 67.13 11.7 2.07 0.15;
    "Kochab" 72_607 222.676_357_5 74.155_503_94 -32.61 11.42 24.91 16.96 2.08 1.47;
    "Almach" 9_640 30.974_801_21 42.329_728_42 42.32 -49.3 8.3 -11.5 2.1 1.2;
    "Tiaki" 112_122 340.666_876_13 -46.884_576_44 135.16 -4.38 18.43 -0.3 2.11 1.62;
    "Algol" 14_576 47.042_218_56 40.955_646_67 2.99 -1.66 36.27 4.0 2.12 -0.05;
    "Denebola" 57_632 177.264_909_76 14.572_058_06 -497.68 -114.67 90.91 -0.2 2.13 0.09;
    "Suhail" 44_816 136.998_991_14 -43.432_590_91 -24.01 13.52 5.99 17.6 2.21 1.65;
    "Mizar" 65_378 200.981_426_17 54.925_359_88 122.467 -22.682 40.213_3 -5.6 2.22 0.051;
    "Sadr" 100_453 305.557_090_98 40.256_679_16 2.39 -0.91 1.78 -5.906 2.23 0.67;
    "Eltanin" 87_833 269.151_541_18 51.488_895_62 -8.48 -22.79 21.14 -27.91 2.23 1.53;
    "Schedar" 3_179 10.126_846_01 56.537_329_22 49.126 -31.595 14.091 -4.204 2.23 1.17;
    "Alphecca" 76_267 233.671_952_03 26.714_685 118.927 -87.711 42.240_8 1.7 2.24 -0.02;
    "Naos" 39_429 120.896_031_41 -40.003_147_8 -29.71 16.68 3.01 -23.9 2.25 -0.27;
    "Aspidiske" 45_556 139.272_528_57 -59.275_232_03 -18.86 11.98 4.26 12.0 2.26 0.18;
    "Caph" 746 2.294_521_58 59.149_781_1 523.5 -179.77 59.58 4.3 2.27 0.34;
    "Larawag" 82_396 252.540_878_39 -34.293_231_59 -614.85 -255.98 51.19 -2.5 2.29 1.16;
    "Dschubba" 78_401 240.083_355_35 -22.621_706_43 -10.21 -35.41 6.64 -6.0 2.32 -0.12;
    "Merak" 53_910 165.460_332_3 56.382_433_65 79.959 32.365 38.603_1 -13.1 2.37 -0.02;
    "Ankaa" 2_081 6.571_047_52 -42.305_987_19 233.05 -356.3 38.5 74.6 2.38 1.09;
    "Enif" 107_315 326.046_483_91 9.875_008_65 26.92 0.44 4.73 3.39 2.39 1.52;
    "Mintaka" 25_930 83.001_667_06 -0.299_095_11 0.64 -0.69 4.71 18.5 2.41 -0.39;
    "Scheat" 113_881 345.943_572_75 28.082_787_12 187.65 136.93 16.64 7.99 2.42 1.67;
    "Sabik" 84_012 257.594_528_71 -15.724_906_64 40.13 99.17 36.91 -2.4 2.42 0.05;
    "Phecda" 58_001 178.457_697_15 53.694_759_73 107.68 11.01 39.21 -11.9 2.44 0.01;
    "Izar" 72_105 221.246_739_83 27.074_222_32 -50.818 21.024 13.826_7 -16.6 2.45 1.16;
    "Aludra" 35_904 111.023_759_5 -29.303_105_51 -4.14 5.81 1.64 41.1 2.45 -0.08;
    "Markab" 113_963 346.190_222_69 15.205_267_15 60.4 -41.3 24.46 -2.7 2.48 -0.04;
    "Aljanah" 102_488 311.552_801_15 33.970_328_34 365.954 308.787 43.176_9 -12.599 2.48 1.04;
    "Acrab" 78_820 241.359_299_93 -19.805_452_78 -5.2 -24.04 8.07 -1.0 2.62 -0.07;
    "Ran" 16_537 53.232_685_38 -9.458_260_97 -974.758 20.876 310.577_3 16.376 3.73 0.88;
    "*  61 Cyg A" 104_214 316.724_748_29 38.749_417_32 4_164.209 3_249.614 285.994_9 -65.82 5.21 1.18;
    "*  61 Cyg B" 104_217 316.730_266_02 38.742_044_03 4_105.976 3_155.942 286.005_4 -64.248 6.03 1.37;
    "Groombridge 1830" 57_939 178.244_863_91 37.718_681_7 4_002.655 -5_817.8 109.029_6 -98.008 6.45 0.75;
    "Lacaille 9352" 114_046 346.466_815_77 -35.853_070_88 6_765.995 1_330.285 304.135_4 8.82 7.39 1.44;
    "Kapteyn's Star" 24_186 77.919_124_33 -45.018_433_82 6_491.223 -5_708.614 254.198_6 245.234 8.853 1.58;
    "Barnard's Star" 87_937 269.452_076_96 4.693_364_97 -801.551 10_362.394 546.975_9 -110.11 9.511 1.729;
    "Proxima Centauri" 70_890 217.428_942_22 -62.679_490_19 -3_781.741 769.465 768.066_5 -20.578 11.13 1.82;
];

/// The rows brighter than a `V` magnitude, brightest first.
///
/// A prefix of [`BRIGHT_STARS`] rather than a filter over it, because the table
/// is sorted.
///
/// ```
/// use corvid_sky::brighter_than;
///
/// // What is left of the sky over a lit city.
/// assert!(brighter_than(1.0).count() >= 8);
/// assert!(brighter_than(-2.0).count() == 0);
/// ```
pub fn brighter_than(magnitude: f64) -> impl Iterator<Item = &'static Star> {
    BRIGHT_STARS
        .iter()
        .take_while(move |star| star.magnitude < magnitude)
}
