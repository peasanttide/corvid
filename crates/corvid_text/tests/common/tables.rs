//! The fixed-shape tables of the test face, as bytes.

/// Big-endian bytes, appended.
///
/// A font file is a big-endian byte stream and nothing else, so the builder is
/// a byte stream and nothing else. Writing it out by hand rather than reaching
/// for a font-building crate is the point: the test face has to be a fact of
/// this repository, identical on every machine and every year, and a downloaded
/// face or a generated one is neither.
#[derive(Default)]
pub(crate) struct Writer(pub(crate) Vec<u8>);

impl Writer {
    /// An empty stream.
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// How many bytes so far.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// One byte.
    pub(crate) fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    /// Two bytes, big-endian.
    pub(crate) fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    /// Two bytes, big-endian, signed.
    pub(crate) fn i16(&mut self, value: i16) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    /// Four bytes, big-endian.
    pub(crate) fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    /// Raw bytes.
    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }

    /// Zero bytes, `count` of them.
    pub(crate) fn zeros(&mut self, count: usize) {
        self.0.resize(self.0.len() + count, 0);
    }

    /// Pad up to a four-byte boundary, as a table directory expects.
    pub(crate) fn pad(&mut self) {
        while !self.0.len().is_multiple_of(4) {
            self.0.push(0);
        }
    }

    /// Overwrite two bytes already written, for a length that was not known
    /// when its field was reached.
    pub(crate) fn patch_u16(&mut self, at: usize, value: u16) {
        self.0[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
}

/// How many units the test face divides its em into.
pub(crate) const EM: u16 = 1000;
/// Baseline to the top of a capital.
pub(crate) const ASCENT: i16 = 800;
/// Baseline to the bottom of a descender.
pub(crate) const DESCENT: i16 = -200;

/// The `head` table: the em grid, the bounding box and the `loca` format.
pub(crate) fn head() -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(0x0001_0000); // version
    w.u32(0x0001_0000); // fontRevision
    w.u32(0); // checkSumAdjustment, which nothing verifies
    w.u32(0x5F0F_3CF5); // magicNumber
    w.u16(0); // flags
    w.u16(EM);
    w.zeros(16); // created and modified
    w.i16(0); // xMin
    w.i16(-200); // yMin
    w.i16(500); // xMax
    w.i16(700); // yMax
    w.u16(0); // macStyle
    w.u16(8); // lowestRecPPEM
    w.i16(2); // fontDirectionHint
    w.i16(1); // indexToLocFormat: long, so `loca` is u32 offsets
    w.i16(0); // glyphDataFormat
    w.pad();
    w.0
}

/// The `hhea` table: the vertical metrics and how many entries `hmtx` has.
pub(crate) fn hhea(glyphs: u16) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(0x0001_0000); // version
    w.i16(ASCENT);
    w.i16(DESCENT);
    w.i16(0); // lineGap
    w.u16(600); // advanceWidthMax
    w.i16(0); // minLeftSideBearing
    w.i16(0); // minRightSideBearing
    w.i16(500); // xMaxExtent
    w.i16(1); // caretSlopeRise
    w.i16(0); // caretSlopeRun
    w.i16(0); // caretOffset
    w.zeros(8); // four reserved
    w.i16(0); // metricDataFormat
    w.u16(glyphs); // numberOfHMetrics: every glyph has its own
    w.pad();
    w.0
}

/// The `maxp` table, version 1.0, of which only the glyph count is read.
pub(crate) fn maxp(glyphs: u16) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(0x0001_0000);
    w.u16(glyphs);
    w.zeros(26);
    w.pad();
    w.0
}

/// The `hmtx` table: one advance and one bearing per glyph.
pub(crate) fn hmtx(advances: &[u16]) -> Vec<u8> {
    let mut w = Writer::new();
    for advance in advances {
        w.u16(*advance);
        w.i16(0); // leftSideBearing, which the outlines already carry
    }
    w.pad();
    w.0
}

/// The legacy `kern` table, format 0: a sorted list of pairs.
///
/// Sorted by `left << 16 | right`, because that is the key the parser binary
/// searches on; an unsorted table parses and then answers nothing.
pub(crate) fn kern(pairs: &[(u16, u16, i16)]) -> Vec<u8> {
    let mut sorted = pairs.to_vec();
    sorted.sort_by_key(|(left, right, _)| (u32::from(*left) << 16) | u32::from(*right));
    let mut w = Writer::new();
    w.u16(0); // version 0, which is what says this is the OpenType variant
    w.u16(1); // one subtable
    w.u16(0); // subtable version
    let length = 14 + sorted.len() * 6;
    w.u16(u16::try_from(length).unwrap_or(u16::MAX));
    w.u8(0); // format 0
    w.u8(0x01); // coverage: horizontal
    w.u16(u16::try_from(sorted.len()).unwrap_or(u16::MAX));
    w.u16(0); // searchRange, which this parser ignores
    w.u16(0); // entrySelector
    w.u16(0); // rangeShift
    for (left, right, value) in sorted {
        w.u16(left);
        w.u16(right);
        w.i16(value);
    }
    w.pad();
    w.0
}

/// A GPOS table holding one `kern` feature, one lookup, and one pair
/// adjustment subtable in format 1.
///
/// This is the shape a face cut this century keeps its kerning in, and it is
/// built here rather than assumed because the code path that reads it is
/// otherwise untested: a face with only a `kern` table proves the fallback and
/// says nothing about the road most fonts take.
pub(crate) fn gpos(first: u16, pairs: &[(u16, i16)]) -> Vec<u8> {
    // The seconds are binary searched, so they have to be in order.
    let mut pairs = pairs.to_vec();
    pairs.sort_by_key(|(second, _)| *second);
    let mut w = Writer::new();
    w.u16(1); // majorVersion
    w.u16(0); // minorVersion
    w.u16(10); // scriptList, right after this ten-byte header
    w.u16(30); // featureList, after the script list's twenty bytes
    w.u16(44); // lookupList, after the feature list's fourteen

    // ScriptList: one script, `DFLT`, with a default language system that names
    // feature zero.
    w.u16(1); // scriptCount
    w.bytes(b"DFLT");
    w.u16(8); // offset to the Script table, from the start of the ScriptList
    w.u16(4); // defaultLangSysOffset
    w.u16(0); // langSysCount
    w.u16(0); // lookupOrderOffset, always zero
    w.u16(0xFFFF); // requiredFeatureIndex, meaning none
    w.u16(1); // featureIndexCount
    w.u16(0); // the one feature

    // FeatureList: one feature, `kern`, naming lookup zero.
    w.u16(1); // featureCount
    w.bytes(b"kern");
    w.u16(8); // offset to the Feature table
    w.u16(0); // featureParamsOffset
    w.u16(1); // lookupIndexCount
    w.u16(0); // the one lookup

    // LookupList: one lookup of type 2, pair adjustment.
    w.u16(1); // lookupCount
    w.u16(4); // offset to the Lookup table
    w.u16(2); // lookupType: pair adjustment
    w.u16(0); // lookupFlag
    w.u16(1); // subTableCount
    w.u16(8); // offset to the subtable, from the start of the Lookup

    // PairPos format 1: one covered first glyph, and a set of seconds under it.
    let subtable = w.len();
    w.u16(1); // posFormat
    let coverage_at = w.len();
    w.u16(0); // coverageOffset, patched once the sets are written
    w.u16(0x0004); // valueFormat1: an x advance and nothing else
    w.u16(0); // valueFormat2: the second glyph is not moved
    w.u16(1); // pairSetCount
    w.u16(12); // pairSetOffset: right after this header

    w.u16(u16::try_from(pairs.len()).unwrap_or(u16::MAX));
    for (second, value) in pairs {
        w.u16(second);
        w.i16(value);
    }

    let coverage = w.len() - subtable;
    w.patch_u16(coverage_at, u16::try_from(coverage).unwrap_or(u16::MAX));
    w.u16(1); // coverageFormat 1: a list of glyphs
    w.u16(1); // glyphCount
    w.u16(first);

    w.pad();
    w.0
}
