//! The index's storage: for one theme (or for the icons outside any
//! theme), every icon name once, in one sorted table, each owning a range
//! of four-byte [`Location`]s. A lookup is a binary search; building one is
//! a handful of allocations, however many files there are.

use std::cmp::Ordering;

use crate::index::Format;

/// How many bits of a [`Location`] hold the format.
const FORMAT_BITS: u32 = 2;
/// How many bits of a [`Location`] hold the base directory.
const BASE_BITS: u32 = 14;

/// How many directories of one theme a [`Location`] can name.
pub(crate) const MAX_DIRECTORIES: usize = 1 << (u32::BITS - BASE_BITS - FORMAT_BITS);
/// How many base directories a [`Location`] can name.
pub(crate) const MAX_BASES: usize = 1 << BASE_BITS;

/// One icon file: in which of the theme's directories (0 for unthemed
/// icons), under which base directory, in which format; packed in four
/// bytes, the directory in the high bits, then the base, then the format,
/// so that their numeric order is by directory, then base, then format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Location(u32);

impl Location {
    /// The location, if `directory` and `base` are within
    /// [`MAX_DIRECTORIES`] and [`MAX_BASES`].
    fn new(directory: usize, base: usize, format: Format) -> Result<Self, Limit> {
        let directory = u32::try_from(directory)
            .ok()
            .filter(|&d| (d as usize) < MAX_DIRECTORIES)
            .ok_or(Limit::Directories)?;
        let base = u32::try_from(base)
            .ok()
            .filter(|&b| (b as usize) < MAX_BASES)
            .ok_or(Limit::Bases)?;
        Ok(Self(
            directory << (BASE_BITS + FORMAT_BITS) | base << FORMAT_BITS | encode(format),
        ))
    }

    /// The theme directory's index in the theme's list.
    pub(crate) fn directory(self) -> usize {
        (self.0 >> (BASE_BITS + FORMAT_BITS)) as usize
    }

    /// The base directory's index in the search path.
    pub(crate) fn base(self) -> usize {
        ((self.0 >> FORMAT_BITS) & ((1 << BASE_BITS) - 1)) as usize
    }

    /// The file's format.
    pub(crate) fn format(self) -> Format {
        decode(self.0)
    }
}

/// A format's two bits, in the order a lookup prefers the formats. The
/// match has no catch-all: a new format does not compile until it is given
/// bits here and in [`decode`], and two bits hold no fifth.
fn encode(format: Format) -> u32 {
    match format {
        Format::Png => 0,
        Format::Svg => 1,
        Format::Xpm => 2,
        Format::Other => 3,
    }
}

/// The format in a location's low two bits: every value of two bits is one
/// of the four, as [`encode`] gives them.
fn decode(bits: u32) -> Format {
    const FORMATS: [Format; 1 << FORMAT_BITS] =
        [Format::Png, Format::Svg, Format::Xpm, Format::Other];
    FORMATS[(bits & ((1 << FORMAT_BITS) - 1)) as usize]
}

/// A limit of the compact form that a theme can pass. What passes it is
/// left out of the index, and said in its problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    /// More directories in one theme than [`MAX_DIRECTORIES`].
    Directories,
    /// More base directories than [`MAX_BASES`].
    Bases,
    /// More bytes of names in one table than a `u32` counts.
    Text,
    /// More files in one table than a `u32` counts.
    Files,
}

impl Limit {
    /// Every limit, for reporting each one passed once.
    const ALL: [Self; 4] = [Self::Directories, Self::Bases, Self::Text, Self::Files];

    /// What the limit counts, for [`crate::ProblemKind::TooLarge`].
    pub(crate) fn what(self) -> &'static str {
        match self {
            Self::Directories => "directories in one theme",
            Self::Bases => "base directories",
            Self::Text => "bytes of icon names in one theme",
            Self::Files => "icon files in one theme",
        }
    }

    /// The largest count allowed.
    pub(crate) fn limit(self) -> u64 {
        match self {
            Self::Directories => MAX_DIRECTORIES as u64,
            Self::Bases => MAX_BASES as u64,
            Self::Text | Self::Files => u64::from(u32::MAX),
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Every icon name of one theme once, sorted by its bytes, each with its
/// locations in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NameTable {
    /// The names, concatenated in order.
    text: Box<str>,
    /// Where each name ends in `text`; it starts where the one before ends.
    ends: Box<[u32]>,
    /// Where each name's locations start in `locations`; one more than the
    /// names, the last being the end of the last name's.
    starts: Box<[u32]>,
    locations: Box<[Location]>,
}

impl NameTable {
    /// The locations of `name`, in order; none when the table lacks it.
    pub(crate) fn get(&self, name: &str) -> &[Location] {
        let (mut low, mut high) = (0, self.ends.len());
        while low < high {
            let middle = low + (high - low) / 2;
            match self.name(middle).as_bytes().cmp(name.as_bytes()) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    let (start, end) = (self.starts[middle], self.starts[middle + 1]);
                    return &self.locations[start as usize..end as usize];
                }
            }
        }
        &[]
    }

    /// How many files the table holds.
    pub(crate) fn files(&self) -> usize {
        self.locations.len()
    }

    /// The `i`th name.
    fn name(&self, i: usize) -> &str {
        let start = if i == 0 { 0 } else { self.ends[i - 1] as usize };
        &self.text[start..self.ends[i] as usize]
    }

    /// Every name, in order.
    #[cfg(test)]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        (0..self.ends.len()).map(|i| self.name(i))
    }
}

/// One file while a table is built: its name in the builder's text, and
/// its location. Twelve bytes, whatever the name.
#[derive(Debug, Clone, Copy)]
struct Record {
    start: u32,
    len: u32,
    at: Location,
}

/// Builds a [`NameTable`] from files given in any order: one buffer for
/// every name given and one vector of records, sorted once at the end.
#[derive(Debug)]
pub(crate) struct TableBuilder {
    text: String,
    records: Vec<Record>,
    /// The largest text and record count allowed: `u32::MAX`, or less in
    /// tests.
    capacity: usize,
    /// The limits passed, one bit each.
    passed: u8,
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self::with_capacity(u32::MAX as usize)
    }
}

impl TableBuilder {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            text: String::new(),
            records: Vec::new(),
            capacity,
            passed: 0,
        }
    }

    /// Adds the file `name` at `directory`, `base` and `format`; or, when
    /// that passes a limit, notes the limit and leaves the file out.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the text and the records are checked against capacity, at most u32::MAX, first"
    )]
    pub(crate) fn push(&mut self, name: &str, directory: usize, base: usize, format: Format) {
        let at = match Location::new(directory, base, format) {
            Ok(at) => at,
            Err(limit) => return self.pass(limit),
        };
        if self.records.len() >= self.capacity {
            return self.pass(Limit::Files);
        }
        if self.text.len() + name.len() > self.capacity {
            return self.pass(Limit::Text);
        }
        let record = Record {
            start: self.text.len() as u32,
            len: name.len() as u32,
            at,
        };
        self.text.push_str(name);
        self.records.push(record);
    }

    /// The limits passed so far, each once.
    pub(crate) fn passed(&self) -> impl Iterator<Item = Limit> + use<> {
        let passed = self.passed;
        Limit::ALL
            .into_iter()
            .filter(move |limit| passed & limit.bit() != 0)
    }

    fn pass(&mut self, limit: Limit) {
        self.passed |= limit.bit();
    }

    /// The table: names sorted by their bytes, each name's locations in
    /// order. Its allocations are exactly its size.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the table holds no more text or locations than the builder, at most u32::MAX of each"
    )]
    pub(crate) fn finish(mut self) -> NameTable {
        // The one empty table, which allocates nothing and equals any other.
        if self.records.is_empty() {
            return NameTable::default();
        }
        let text = self.text;
        let name = |record: &Record| &text.as_bytes()[record.start as usize..][..record.len as usize];
        self.records
            .sort_unstable_by(|a, b| name(a).cmp(name(b)).then(a.at.cmp(&b.at)));
        let mut names = 0;
        let mut bytes = 0;
        for (i, record) in self.records.iter().enumerate() {
            if i == 0 || name(&self.records[i - 1]) != name(record) {
                names += 1;
                bytes += record.len as usize;
            }
        }
        let mut sorted = String::with_capacity(bytes);
        let mut ends = Vec::with_capacity(names);
        let mut starts = Vec::with_capacity(names + 1);
        let mut locations = Vec::with_capacity(self.records.len());
        for (i, record) in self.records.iter().enumerate() {
            if i == 0 || name(&self.records[i - 1]) != name(record) {
                let start = record.start as usize;
                sorted.push_str(&text[start..start + record.len as usize]);
                ends.push(sorted.len() as u32);
                starts.push(locations.len() as u32);
            }
            locations.push(record.at);
        }
        starts.push(locations.len() as u32);
        NameTable {
            text: sorted.into_boxed_str(),
            ends: ends.into_boxed_slice(),
            starts: starts.into_boxed_slice(),
            locations: locations.into_boxed_slice(),
        }
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
