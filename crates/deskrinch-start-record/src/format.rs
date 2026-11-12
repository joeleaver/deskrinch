//! The file's format: TOML, a version, then one `[[start]]` table per
//! application, best first.
//!
//! ```toml
//! version = 1
//!
//! [[start]]
//! id = "org.mozilla.firefox.desktop"
//! score = 9.58
//! last = 1731400000
//! ```
//!
//! Reading is lenient where it can be: a `[[start]]` that is not a valid
//! one (an ID that is not a desktop entry ID, a score that is not finite
//! and positive, a time before the epoch, a key missing) is left out and
//! counted, and the rest are read. A file that is not TOML, or has no
//! integer `version`, or whose `start` is not an array of tables, is not a
//! record at all. A newer version is left for the version that wrote it.

use deskrinch_desktop_entries::DesktopId;
use serde::{Deserialize, Serialize};

use crate::record::{Record, Starts};

/// The version this crate reads and writes.
pub const VERSION: i64 = 1;

/// What a file's text was.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Parsed {
    /// A record, with the number of `[[start]]` tables left out as invalid.
    Record { record: Record, dropped: usize },
    /// Not a record of starts: why.
    Damaged(String),
    /// A record of a version newer than [`VERSION`].
    Newer(i64),
}

/// One `[[start]]` as the file has it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Line {
    id: String,
    score: f64,
    last: u64,
}

/// The whole file, as written.
#[derive(Debug, Serialize)]
struct File {
    version: i64,
    start: Vec<Line>,
}

/// Reads a file's `text`.
pub(crate) fn parse(text: &str) -> Parsed {
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => return Parsed::Damaged(format!("not TOML: {}", error.message())),
    };
    let version = match table.get("version") {
        Some(toml::Value::Integer(version)) => *version,
        Some(_) => return Parsed::Damaged("its version is not an integer".to_owned()),
        None => return Parsed::Damaged("it has no version".to_owned()),
    };
    if version > VERSION {
        return Parsed::Newer(version);
    }
    if version < VERSION {
        return Parsed::Damaged(format!("it has an unknown version, {version}"));
    }
    let lines = match table.get("start") {
        None => &Vec::new(),
        Some(toml::Value::Array(lines)) => lines,
        Some(_) => return Parsed::Damaged("its `start` is not an array of tables".to_owned()),
    };
    let mut record = Record::new();
    let mut dropped = 0;
    for value in lines {
        match entry(value.clone()) {
            Some((id, starts)) if record.get(&id).is_none() => record.insert(id, starts),
            // Invalid, or a second one for the same application.
            _ => dropped += 1,
        }
    }
    Parsed::Record { record, dropped }
}

/// One valid `[[start]]`.
fn entry(value: toml::Value) -> Option<(DesktopId, Starts)> {
    let line: Line = value.try_into().ok()?;
    let id = DesktopId::new(line.id)?;
    let valid = line.score.is_finite() && line.score > 0.0;
    valid.then_some((
        id,
        Starts {
            score: line.score,
            last: line.last,
        },
    ))
}

/// The text of `record`'s file, best first.
pub(crate) fn write(record: &Record) -> Result<String, toml::ser::Error> {
    let file = File {
        version: VERSION,
        start: record
            .ranked()
            .map(|(id, starts)| Line {
                id: id.as_str().to_owned(),
                score: starts.score,
                last: starts.last,
            })
            .collect(),
    };
    toml::to_string(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DesktopId {
        DesktopId::new(format!("{name}.desktop")).unwrap()
    }

    #[test]
    fn a_record_reads_back_as_it_was_written() {
        let mut record = Record::new();
        record.record(&id("a"), 1_000);
        record.record(&id("b"), 2_000);
        record.record(&id("b"), 3_000);
        let text = write(&record).unwrap();
        assert!(text.starts_with("version = 1\n"), "{text}");
        // Best first.
        assert!(text.find("b.desktop") < text.find("a.desktop"), "{text}");
        assert_eq!(parse(&text), Parsed::Record { record, dropped: 0 });
    }

    #[test]
    fn an_empty_record_is_a_version_alone() {
        let text = write(&Record::new()).unwrap();
        assert_eq!(
            parse(&text),
            Parsed::Record {
                record: Record::new(),
                dropped: 0
            }
        );
    }

    #[test]
    fn invalid_starts_are_left_out_and_the_rest_read() {
        let text = "version = 1\n\
            [[start]]\nid = \"good.desktop\"\nscore = 2.0\nlast = 5\n\
            [[start]]\nid = \"no-suffix\"\nscore = 1.0\nlast = 5\n\
            [[start]]\nid = \"zero.desktop\"\nscore = 0.0\nlast = 5\n\
            [[start]]\nid = \"nan.desktop\"\nscore = nan\nlast = 5\n\
            [[start]]\nid = \"inf.desktop\"\nscore = inf\nlast = 5\n\
            [[start]]\nid = \"neg.desktop\"\nscore = 1.0\nlast = -5\n\
            [[start]]\nid = \"missing.desktop\"\nscore = 1.0\n\
            [[start]]\nid = \"extra.desktop\"\nscore = 1.0\nlast = 5\nmore = 1\n\
            [[start]]\nid = \"good.desktop\"\nscore = 9.0\nlast = 9\n";
        let Parsed::Record { record, dropped } = parse(text) else {
            panic!("{:?}", parse(text));
        };
        assert_eq!(dropped, 8);
        assert_eq!(record.len(), 1);
        assert_eq!(
            record.get(&id("good")),
            Some(Starts {
                score: 2.0,
                last: 5
            })
        );
    }

    #[test]
    fn what_is_not_a_record_is_damaged() {
        for text in [
            "\0\u{1}binary",
            "[[start]]\nid = \"a.desktop\"\nscore = 1.0\nlast = 5\n",
            "version = \"1\"\n",
            "version = 0\n",
            "version = 1\nstart = 3\n",
            "{\"version\": 1}",
        ] {
            assert!(matches!(parse(text), Parsed::Damaged(_)), "{text:?}");
        }
    }

    #[test]
    fn a_newer_version_is_named() {
        assert_eq!(
            parse("version = 2\nwhatever = true\n"),
            Parsed::Newer(2)
        );
    }
}
