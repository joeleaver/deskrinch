//! Building tables, and the builder's limits, with made-up indices.

use super::*;
use crate::table::{MAX_BASES, MAX_DIRECTORIES};

fn locations(table: &NameTable, name: &str) -> Vec<(usize, usize, Format)> {
    table
        .get(name)
        .iter()
        .map(|at| (at.directory(), at.base(), at.format()))
        .collect()
}

#[test]
fn a_directory_past_the_limit_is_left_out_and_reported() {
    let mut builder = TableBuilder::default();
    builder.push("a", MAX_DIRECTORIES - 1, 0, Format::Png);
    builder.push("a", MAX_DIRECTORIES, 0, Format::Png);
    builder.push("b", usize::MAX, 0, Format::Png);
    assert_eq!(builder.passed().collect::<Vec<_>>(), [Limit::Directories]);
    let table = builder.finish();
    assert_eq!(
        locations(&table, "a"),
        [(MAX_DIRECTORIES - 1, 0, Format::Png)]
    );
    assert_eq!(locations(&table, "b"), []);
}

#[test]
fn a_base_past_the_limit_is_left_out_and_reported() {
    let mut builder = TableBuilder::default();
    builder.push("a", 3, MAX_BASES - 1, Format::Svg);
    builder.push("a", 3, MAX_BASES, Format::Svg);
    assert_eq!(builder.passed().collect::<Vec<_>>(), [Limit::Bases]);
    assert_eq!(
        locations(&builder.finish(), "a"),
        [(3, MAX_BASES - 1, Format::Svg)]
    );
}

#[test]
fn text_and_files_past_the_capacity_are_left_out_and_reported() {
    let mut builder = TableBuilder::with_capacity(5);
    builder.push("abc", 0, 0, Format::Png);
    builder.push("de", 0, 0, Format::Png);
    builder.push("f", 0, 0, Format::Png);
    assert_eq!(builder.passed().collect::<Vec<_>>(), [Limit::Text]);
    let mut builder = TableBuilder::with_capacity(2);
    for _ in 0..3 {
        builder.push("", 0, 0, Format::Png);
    }
    assert_eq!(builder.passed().collect::<Vec<_>>(), [Limit::Files]);
    assert_eq!(builder.finish().files(), 2);
}

#[test]
fn names_are_sorted_by_bytes_with_their_locations_in_order() {
    let mut builder = TableBuilder::default();
    for (name, directory, base, format) in [
        ("folder-open", 2, 0, Format::Svg),
        ("folder", 1, 1, Format::Png),
        ("folder", 0, 1, Format::Xpm),
        ("folder", 0, 1, Format::Png),
        ("Folder", 0, 0, Format::Png),
        ("folder", 0, 0, Format::Svg),
        ("é", 0, 0, Format::Png),
    ] {
        builder.push(name, directory, base, format);
    }
    assert_eq!(builder.passed().count(), 0);
    let table = builder.finish();
    assert_eq!(
        table.names().collect::<Vec<_>>(),
        ["Folder", "folder", "folder-open", "é"]
    );
    assert_eq!(
        locations(&table, "folder"),
        [
            (0, 0, Format::Svg),
            (0, 1, Format::Png),
            (0, 1, Format::Xpm),
            (1, 1, Format::Png)
        ]
    );
    assert_eq!(locations(&table, "folder-open"), [(2, 0, Format::Svg)]);
    // A prefix of a name is not that name, nor is a name it prefixes.
    assert_eq!(locations(&table, "fold"), []);
    assert_eq!(locations(&table, "folder-open-x"), []);
    assert_eq!(locations(&table, ""), []);
    assert_eq!(table.files(), 7);
}

#[test]
fn an_empty_builder_gives_the_empty_table() {
    let table = TableBuilder::default().finish();
    assert_eq!(table, NameTable::default());
    assert_eq!(table.get("a"), []);
    assert_eq!(table.files(), 0);
}

#[test]
fn tables_of_the_same_files_are_equal_whatever_the_order_given() {
    let files = [
        ("b", 1, 0, Format::Png),
        ("a", 0, 1, Format::Svg),
        ("a", 0, 0, Format::Png),
    ];
    let build = |files: &mut dyn Iterator<Item = &(&str, usize, usize, Format)>| {
        let mut builder = TableBuilder::default();
        for &(name, directory, base, format) in files {
            builder.push(name, directory, base, format);
        }
        builder.finish()
    };
    let forward = build(&mut files.iter());
    assert_eq!(forward, build(&mut files.iter().rev()));
    let fewer = build(&mut files.iter().skip(1));
    assert_ne!(forward, fewer);
    let moved = build(&mut [("b", 1, 0, Format::Svg), files[1], files[2]].iter());
    assert_ne!(forward, moved);
}
