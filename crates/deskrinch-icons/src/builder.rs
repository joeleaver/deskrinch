//! Building a [`NameTable`] from files given in any order, without an
//! allocation per name or per file: each distinct name is interned once
//! into one buffer (a `hashbrown::HashTable` of name ids, keyed by the
//! names in the buffer), each file is an eight-byte record of its name's id
//! and its location, and at the end the distinct names are sorted by their
//! bytes and the records by an integer key.

use std::hash::BuildHasher;

use hashbrown::hash_table::Entry;
use hashbrown::{DefaultHashBuilder, HashTable};

use crate::index::Format;
use crate::table::{Limit, Location, NameTable};

/// One file while a table is built: its name's id, and its location.
#[derive(Debug, Clone, Copy)]
struct Record {
    name: u32,
    at: Location,
}

/// The names given so far, each once, in the order first given.
#[derive(Debug, Default)]
struct Names {
    text: String,
    /// Where each name ends in `text`; it starts where the one before ends.
    ends: Vec<u32>,
}

impl Names {
    fn get(&self, id: u32) -> &str {
        let id = id as usize;
        let start = if id == 0 {
            0
        } else {
            self.ends[id - 1] as usize
        };
        &self.text[start..self.ends[id] as usize]
    }
}

/// Builds a [`NameTable`]; see the module's description.
#[derive(Debug)]
pub(crate) struct TableBuilder {
    names: Names,
    /// The names' ids, found by the hash of the name.
    ids: HashTable<u32>,
    hasher: DefaultHashBuilder,
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
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            names: Names::default(),
            ids: HashTable::new(),
            hasher: DefaultHashBuilder::default(),
            records: Vec::new(),
            capacity,
            passed: 0,
        }
    }

    /// Adds the file `name` at `directory`, `base` and `format`; or, when
    /// that passes a limit, notes the limit and leaves the file out.
    pub(crate) fn push(&mut self, name: &str, directory: usize, base: usize, format: Format) {
        let at = match Location::new(directory, base, format) {
            Ok(at) => at,
            Err(limit) => return self.pass(limit),
        };
        if self.records.len() >= self.capacity {
            return self.pass(Limit::Files);
        }
        match self.intern(name) {
            Some(name) => self.records.push(Record { name, at }),
            None => self.pass(Limit::Text),
        }
    }

    /// The id of `name`, added if new; `None` when adding it would pass the
    /// text's capacity.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the ids are fewer than the records and the text is checked against capacity, \
                  each at most u32::MAX"
    )]
    fn intern(&mut self, name: &str) -> Option<u32> {
        let Self {
            names, ids, hasher, ..
        } = self;
        let hash = hasher.hash_one(name);
        let entry = ids.entry(
            hash,
            |&id| names.get(id) == name,
            |&id| hasher.hash_one(names.get(id)),
        );
        match entry {
            Entry::Occupied(found) => Some(*found.get()),
            Entry::Vacant(vacant) => {
                if names.text.len() + name.len() > self.capacity {
                    return None;
                }
                let id = names.ends.len() as u32;
                vacant.insert(id);
                names.text.push_str(name);
                names.ends.push(names.text.len() as u32);
                Some(id)
            }
        }
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
        reason = "ids, text and locations are each within u32::MAX, as push keeps them"
    )]
    pub(crate) fn finish(self) -> NameTable {
        // The one empty table, which allocates nothing and equals any other.
        if self.records.is_empty() {
            return NameTable::default();
        }
        let Self {
            names, mut records, ..
        } = self;
        let count = names.ends.len();
        // The names by their bytes, and each id's place among them.
        let mut order: Vec<u32> = (0..count as u32).collect();
        order.sort_unstable_by(|&a, &b| names.get(a).as_bytes().cmp(names.get(b).as_bytes()));
        let mut rank = vec![0_u32; count];
        for (place, &id) in order.iter().enumerate() {
            rank[id as usize] = place as u32;
        }
        records.sort_unstable_by_key(|record| (rank[record.name as usize], record.at));

        let mut text = String::with_capacity(names.text.len());
        let mut ends = Vec::with_capacity(count);
        for &id in &order {
            text.push_str(names.get(id));
            ends.push(text.len() as u32);
        }
        let mut starts = Vec::with_capacity(count + 1);
        let mut locations = Vec::with_capacity(records.len());
        for (i, record) in records.iter().enumerate() {
            if i == 0 || records[i - 1].name != record.name {
                starts.push(locations.len() as u32);
            }
            locations.push(record.at);
        }
        starts.push(locations.len() as u32);
        NameTable::from_parts(
            text.into_boxed_str(),
            ends.into_boxed_slice(),
            starts.into_boxed_slice(),
            locations.into_boxed_slice(),
        )
    }
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
