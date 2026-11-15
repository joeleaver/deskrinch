//! A location's packing and its limits, with made-up indices; the empty
//! table.

use super::*;

#[test]
fn a_location_keeps_its_parts_and_orders_as_directory_then_base_then_format() {
    let largest = Location::new(MAX_DIRECTORIES - 1, MAX_BASES - 1, Format::Other).unwrap();
    assert_eq!(
        (largest.directory(), largest.base(), largest.format()),
        (MAX_DIRECTORIES - 1, MAX_BASES - 1, Format::Other)
    );
    let mut all = Vec::new();
    for directory in [0, 1, 7, MAX_DIRECTORIES - 1] {
        for base in [0, 1, 300, MAX_BASES - 1] {
            for format in [Format::Png, Format::Svg, Format::Xpm, Format::Other] {
                let at = Location::new(directory, base, format).unwrap();
                assert_eq!(
                    (at.directory(), at.base(), at.format()),
                    (directory, base, format)
                );
                all.push(((directory, base, format), at));
            }
        }
    }
    let mut by_parts = all.clone();
    by_parts.sort_by_key(|(parts, _)| *parts);
    all.sort_by_key(|(_, at)| *at);
    assert_eq!(all, by_parts);
}

#[test]
fn a_location_past_a_limit_is_refused() {
    let directory = Location::new(MAX_DIRECTORIES, 0, Format::Png);
    assert_eq!(directory, Err(Limit::Directories));
    assert_eq!(Location::new(0, MAX_BASES, Format::Png), Err(Limit::Bases));
    let both = Location::new(usize::MAX, usize::MAX, Format::Png);
    assert_eq!(both, Err(Limit::Directories));
}

#[test]
fn the_empty_table_has_no_name() {
    let table = NameTable::default();
    assert_eq!(table.get(""), []);
    assert_eq!(table.files(), 0);
    assert_eq!(table.names().count(), 0);
}
