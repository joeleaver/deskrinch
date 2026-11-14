//! rinch's `RINCH_PERF` lines, every kind, as rinch writes them (rinch
//! `crates/rinch/src/window_perf.rs` and rinch-dom's `perf.rs`, at the
//! commit the workspace pins):
//!
//! - `[PERF] frame N: …` and `[PERF] prepared N: …`: the main window's
//!   presented and prepared frames, each numbered in its own sequence.
//! - `[PERF] window <id> frame N: …` and `[PERF] window <id> prepared N: …`:
//!   another window's, `<id>` its `WindowHandle::id`.
//! - `[PERF] window <id> opened: title="…" app_id="…" output="…"
//!   output_id=<n>`: a window other than the main one, named when it is
//!   made. The values are quoted as Rust's `{:?}` writes a string; `app_id`
//!   and `output` are left out when its props name none, and `output_id` is
//!   there only for a layer surface placed by an output's id.
//! - `[PERF] window <id> closed`: a window closed, the main one included
//!   (whose id is in no other line).
//!
//! [`parse`] reads one line. The frame summary after the colon is not read.

/// Which window a line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PerfWindow {
    /// The app's main window, whose lines carry no id.
    Main,
    /// Another window, by its id.
    Id(u64),
}

/// What a window's `opened` line says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// Its id.
    pub id: u64,
    /// Its title.
    pub title: String,
    /// Its app id, when its props name one.
    pub app_id: Option<String>,
    /// The output its props name (or, placed by an output's id, that
    /// output's name, when it has one).
    pub output: Option<String>,
    /// The id of the output its layer surface is placed by, if it is.
    pub output_id: Option<u64>,
}

/// One `RINCH_PERF` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfLine {
    /// A presented frame.
    Frame {
        /// Whose.
        window: PerfWindow,
        /// Its number in that window's presented frames, from 1.
        n: u64,
    },
    /// A prepared frame (painted while hidden, not presented).
    Prepared {
        /// Whose.
        window: PerfWindow,
        /// Its number in that window's prepared frames, from 1.
        n: u64,
    },
    /// A window was made.
    Opened(Opened),
    /// A window closed.
    Closed {
        /// Its id.
        id: u64,
    },
}

/// The `RINCH_PERF` line `line` is, if it is one (ANSI codes ignored).
#[must_use]
pub fn parse(line: &str) -> Option<PerfLine> {
    let line = crate::log::strip_ansi(line);
    let rest = line.trim_end().strip_prefix("[PERF] ")?;
    let (window, rest) = match rest.strip_prefix("window ") {
        Some(rest) => {
            let (id, rest) = rest.split_once(' ')?;
            (PerfWindow::Id(id.parse().ok()?), rest)
        }
        None => (PerfWindow::Main, rest),
    };
    if let Some(n) = counted(rest, "frame ") {
        return Some(PerfLine::Frame { window, n });
    }
    if let Some(n) = counted(rest, "prepared ") {
        return Some(PerfLine::Prepared { window, n });
    }
    let PerfWindow::Id(id) = window else {
        return None;
    };
    if rest == "closed" {
        return Some(PerfLine::Closed { id });
    }
    opened(id, rest.strip_prefix("opened: ")?).map(PerfLine::Opened)
}

/// `N` of `<kind>N: …`.
fn counted(rest: &str, kind: &str) -> Option<u64> {
    rest.strip_prefix(kind)?.split(':').next()?.parse().ok()
}

/// The fields of an `opened` line after `opened: `.
fn opened(id: u64, mut fields: &str) -> Option<Opened> {
    let mut opened = Opened {
        id,
        title: String::new(),
        app_id: None,
        output: None,
        output_id: None,
    };
    let mut titled = false;
    while !fields.is_empty() {
        let (key, rest) = fields.split_once('=')?;
        let (value, rest) = if rest.starts_with('"') {
            quoted(rest)?
        } else {
            let end = rest.find(' ').unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        match key {
            "title" => {
                opened.title = value;
                titled = true;
            }
            "app_id" => opened.app_id = Some(value),
            "output" => opened.output = Some(value),
            "output_id" => opened.output_id = Some(value.parse().ok()?),
            // A field a later rinch adds is not this one's to read.
            _ => {}
        }
        fields = rest.trim_start();
    }
    titled.then_some(opened)
}

/// The string `text` starts with, quoted as Rust's `{:?}` writes one, and
/// what follows it.
fn quoted(text: &str) -> Option<(String, &str)> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut value = String::new();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((value, &text[at + 1..])),
            '\\' => match chars.next()?.1 {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                '0' => value.push('\0'),
                '\\' => value.push('\\'),
                '"' => value.push('"'),
                '\'' => value.push('\''),
                'u' => {
                    if chars.next()?.1 != '{' {
                        return None;
                    }
                    let mut hex = String::new();
                    loop {
                        match chars.next()?.1 {
                            '}' => break,
                            digit => hex.push(digit),
                        }
                    }
                    value.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            },
            other => value.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a rinch program with a hidden main window it prepares and then
    /// shows, a window per output (two outputs) placed by output id, and a
    /// hidden window it prepares wrote under `RINCH_PERF` in a nested sway,
    /// built against rinch f2a2712e9b4d18676ff07ee784c3aaf88820c6ec.
    const FIXTURE: &str = include_str!("../tests/fixtures/two-windows-and-a-prepared-one.txt");

    #[test]
    fn every_line_of_a_real_run_is_read() {
        let lines: Vec<PerfLine> = FIXTURE.lines().filter_map(parse).collect();
        let title = "a \"quoted\" \\ title";
        let per_output = |id, output: &str, output_id| {
            PerfLine::Opened(Opened {
                id,
                title: title.to_owned(),
                app_id: Some("org.example.fixture".to_owned()),
                output: Some(output.to_owned()),
                output_id: Some(output_id),
            })
        };
        let main = PerfWindow::Main;
        let (two, three, four) = (PerfWindow::Id(2), PerfWindow::Id(3), PerfWindow::Id(4));
        assert_eq!(
            lines,
            [
                PerfLine::Prepared { window: main, n: 1 },
                per_output(2, "HEADLESS-2", 51),
                per_output(3, "HEADLESS-1", 52),
                PerfLine::Opened(Opened {
                    id: 4,
                    title: "hidden".to_owned(),
                    app_id: None,
                    output: None,
                    output_id: None,
                }),
                PerfLine::Prepared { window: four, n: 1 },
                PerfLine::Frame {
                    window: three,
                    n: 1
                },
                PerfLine::Frame { window: two, n: 1 },
                PerfLine::Frame {
                    window: three,
                    n: 2
                },
                PerfLine::Frame { window: two, n: 2 },
                PerfLine::Frame { window: main, n: 1 },
                PerfLine::Frame { window: main, n: 2 },
                PerfLine::Closed { id: 3 },
                PerfLine::Closed { id: 2 },
                PerfLine::Frame { window: main, n: 3 },
                PerfLine::Closed { id: 4 },
            ]
        );
        // The other lines (the session's, winit's) are none.
        assert_eq!(
            FIXTURE.lines().filter(|l| l.contains("[PERF]")).count(),
            lines.len()
        );
    }

    #[test]
    fn an_output_without_a_name_and_escapes_of_every_kind() {
        // As rinch's own tests write them.
        assert_eq!(
            parse(r#"[PERF] window 3 opened: title="Panel" output_id=52"#),
            Some(PerfLine::Opened(Opened {
                id: 3,
                title: "Panel".to_owned(),
                app_id: None,
                output: None,
                output_id: Some(52),
            }))
        );
        let Some(PerfLine::Opened(opened)) =
            parse(r#"[PERF] window 9 opened: title="a\tb\nc \u{1b} é" app_id="x y""#)
        else {
            panic!();
        };
        assert_eq!(opened.title, "a\tb\nc \u{1b} é");
        assert_eq!(opened.app_id.as_deref(), Some("x y"));
    }

    #[test]
    fn what_is_not_a_perf_line_is_none() {
        for line in [
            "2026-11-14T18:18:29Z  WARN winit: [PERF] frame 1: in a message",
            "[PERF] frame x: style",
            "[PERF] window x frame 1: style",
            "[PERF] closed",
            r#"[PERF] window 3 opened: app_id="no title""#,
            r#"[PERF] window 3 opened: title="unterminated"#,
        ] {
            assert_eq!(parse(line), None, "{line}");
        }
        assert_eq!(
            parse("\u{1b}[2m[PERF] window 5 closed\u{1b}[0m"),
            Some(PerfLine::Closed { id: 5 })
        );
    }
}
