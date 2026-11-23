//! `--start-ends WHEN`: where a measured start ends, and with it what the
//! figures "to first frame" run to and when the settle and the idle window
//! begin.
//!
//! - `first-frame` (the default): the main window's first frame, or with
//!   `--windows N` each window's; the rows keep their names ("CPU to first
//!   frame").
//! - `frame:N`: the main window's, or each expected window's, Nth presented
//!   frame. It is sound only for a program whose frames come in a fixed
//!   order: the wallpaper's picture is in its first frame or its second by
//!   the decode's timing, so its end is the frame that painted the picture.
//! - `counter:NAME`: the main window's, or each expected window's, first
//!   presented frame whose rinch frame line carries the counter NAME above
//!   zero: the frame that did a given piece of work, whichever frame it was.
//!   The wallpaper's picture shown is `counter:image_draws`, once the pin has
//!   rinch's `image_draws` counter (rinch issue 2769).
//! - `line:TEXT`: the first line of the program's output that contains
//!   TEXT.
//! - `ready`: the readiness `deskrinch-shell` declares for the component
//!   the program is (by its file name, or as `--component` names it):
//!   owning its bus name on the session's bus. A component that is ready
//!   once started (nothing waits for it) has no such end, and is refused.
//! - `bus-name:NAME`: owning NAME on the session's bus, for a program that
//!   is not one of the shell's components.
//!
//! Any end but the default names its rows ("CPU to start-up's end") and
//! the header says which end it was. A program that opened no window in
//! any run is said so in the header, so that its frame rows of 0 are not
//! read as a failure to draw.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use deskrinch_shell::Ready;
use deskrinch_test_component::{PerfLine, PerfWindow, Watch, perf};

use super::quiet::Neighbours;
use super::windows::{self, Expect};
use crate::error::Error;

/// What `--start-ends` asks for, as parsed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum StartEnds {
    /// `first-frame`.
    #[default]
    FirstFrame,
    /// `frame:N`.
    Frame(u64),
    /// `counter:NAME`.
    Counter(String),
    /// `line:TEXT`.
    Line(String),
    /// `ready`.
    Ready,
    /// `bus-name:NAME`.
    BusName(String),
}

impl StartEnds {
    /// One of each end, for the forms the help and the errors list.
    pub(crate) const EACH: [Self; 6] = [
        Self::FirstFrame,
        Self::Frame(1),
        Self::Counter(String::new()),
        Self::Line(String::new()),
        Self::Ready,
        Self::BusName(String::new()),
    ];

    /// How it is written on the command line, as the help names it.
    pub(crate) fn form(&self) -> &'static str {
        match self {
            Self::FirstFrame => "first-frame",
            Self::Frame(_) => "frame:N",
            Self::Counter(_) => "counter:NAME",
            Self::Line(_) => "line:TEXT",
            Self::Ready => "ready",
            Self::BusName(_) => "bus-name:NAME",
        }
    }

    /// The forms of the ends `keep` takes, joined as a list: `a, b or c`.
    pub(crate) fn forms(keep: impl Fn(&Self) -> bool) -> String {
        let forms: Vec<&str> = Self::EACH
            .iter()
            .filter(|end| keep(end))
            .map(Self::form)
            .collect();
        match forms.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
            Some((last, _)) => (*last).to_owned(),
            None => String::new(),
        }
    }

    /// `--start-ends`' value `text`.
    ///
    /// # Errors
    ///
    /// For anything but one of the forms [`Self::form`] names: `frame:N`
    /// with N at least 1, and `counter:NAME`, `line:TEXT` and
    /// `bus-name:NAME` with a value that is not empty.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let refused = || format!("{text:?}: give {}", Self::forms(|_| true));
        match text {
            "first-frame" => return Ok(Self::FirstFrame),
            "ready" => return Ok(Self::Ready),
            _ => {}
        }
        let (kind, value) = text.split_once(':').ok_or_else(refused)?;
        match (kind, value) {
            ("frame", n) => n
                .parse()
                .ok()
                .filter(|&n| n >= 1)
                .map(Self::Frame)
                .ok_or_else(refused),
            ("counter", name) if !name.is_empty() => Ok(Self::Counter(name.to_owned())),
            ("line", line) if !line.is_empty() => Ok(Self::Line(line.to_owned())),
            ("bus-name", name) if !name.is_empty() => Ok(Self::BusName(name.to_owned())),
            _ => Err(refused()),
        }
    }
}

/// Where a run's start-up ends, resolved for the program measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum End {
    /// The Nth presented frame (of the main window, or of each expected
    /// window); 1 is the default end.
    Frame(u64),
    /// The first presented frame (of the main window, or of each expected
    /// window) whose frame line carries this counter above zero.
    Counter(String),
    /// The first output line containing this text.
    Line(String),
    /// Owning this name on the session's bus; `shell` names the component
    /// whose readiness it is, when it came from `ready`.
    BusName {
        /// The name.
        name: String,
        /// The component, for `ready`.
        shell: Option<&'static str>,
    },
}

impl End {
    /// The end `asked` names for `program` (`component` its `--component`).
    ///
    /// # Errors
    ///
    /// For `ready`, when the program is none of the shell's components, or
    /// is one that is ready once started.
    pub(crate) fn resolve(
        asked: &StartEnds,
        program: &Path,
        component: Option<&str>,
    ) -> Result<Self, Error> {
        Ok(match asked {
            StartEnds::FirstFrame => Self::Frame(1),
            StartEnds::Frame(n) => Self::Frame(*n),
            StartEnds::Counter(name) => Self::Counter(name.clone()),
            StartEnds::Line(text) => Self::Line(text.clone()),
            StartEnds::BusName(name) => Self::BusName {
                name: name.clone(),
                shell: None,
            },
            StartEnds::Ready => {
                let Some(component) = crate::grant::component(program, component)? else {
                    return Err(Error::NoReadiness {
                        program: program.display().to_string(),
                        why: "it is none of the shell's components by its file name, and \
                              --component names none"
                            .to_owned(),
                    });
                };
                match component.ready {
                    Ready::BusName(name) => Self::BusName {
                        name: name.to_owned(),
                        shell: Some(component.package),
                    },
                    Ready::Started => {
                        return Err(Error::NoReadiness {
                            program: program.display().to_string(),
                            why: format!(
                                "deskrinch-shell has {} ready once started, which no moment \
                                 marks; use {}",
                                component.package,
                                StartEnds::forms(|end| *end != StartEnds::Ready)
                            ),
                        });
                    }
                }
            }
        })
    }

    /// Whether it is the default end, whose rows keep their names.
    pub(crate) fn is_first_frame(&self) -> bool {
        *self == Self::Frame(1)
    }

    /// What the rows run to: "first frame", or "start-up's end".
    pub(crate) fn to(&self) -> &'static str {
        if self.is_first_frame() {
            "first frame"
        } else {
            "start-up's end"
        }
    }

    /// The same with its article: "the first frame", or "start-up's end".
    pub(crate) fn at(&self) -> &'static str {
        if self.is_first_frame() {
            "the first frame"
        } else {
            "start-up's end"
        }
    }

    /// Where start-up ends, for the header, with `expect`'s windows.
    pub(crate) fn describe(&self, expect: Expect) -> String {
        match self {
            Self::Frame(n) => {
                let nth = ordinal(*n);
                match expect {
                    Expect::Main => format!("the main window's {nth} frame"),
                    Expect::Windows(_) => format!("each window's {nth} frame"),
                }
            }
            Self::Counter(name) => match expect {
                Expect::Main => format!("the main window's first frame with {name}"),
                Expect::Windows(_) => format!("each window's first frame with {name}"),
            },
            Self::Line(text) => format!("the first line of its output with {text:?}"),
            Self::BusName { name, shell: None } => {
                format!("owning {name} on the session's bus")
            }
            Self::BusName {
                name,
                shell: Some(package),
            } => format!(
                "owning {name} on the session's bus (the readiness deskrinch-shell declares for \
                 {package})"
            ),
        }
    }
}

/// The window whose presented frame `line` is, if its frame line carries
/// the counter `name` above zero.
fn counted_frame(line: &str, name: &str) -> Option<PerfWindow> {
    let PerfLine::Frame { window, .. } = perf::parse(line)? else {
        return None;
    };
    (perf::counter(line, name)? > 0).then_some(window)
}

/// The windows that have presented a frame carrying the counter `name`
/// above zero.
fn counted_windows(watch: &Watch, name: &str) -> BTreeSet<PerfWindow> {
    watch
        .lines()
        .iter()
        .filter_map(|line| counted_frame(line, name))
        .collect()
}

/// `n` as an ordinal: 1st, 2nd, 3rd, 4th, ..., 11th, 12th, 13th, 21st.
fn ordinal(n: u64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// What one run's start waits for: the windows whose frames are read, the
/// end, and the session's bus (for an end on it).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Start<'a> {
    /// Which windows' frames are read.
    pub(crate) expect: Expect,
    /// Where start-up ends.
    pub(crate) end: &'a End,
    /// The session's bus address.
    pub(crate) bus: &'a str,
    /// The session's processes the program may wait on, which its settles
    /// watch too.
    pub(crate) neighbours: Neighbours,
}

impl Start<'_> {
    /// Whether start-up has ended.
    pub(crate) fn ended(&self, watch: &Watch) -> bool {
        match self.end {
            End::Frame(1) => self.expect.started(watch),
            End::Frame(n) => match self.expect {
                Expect::Main => watch.wait_main_frames(*n, Duration::ZERO),
                Expect::Windows(windows) => watch.wait_frames_each(windows, *n, Duration::ZERO),
            },
            End::Counter(name) => {
                let counted = counted_windows(watch, name);
                match self.expect {
                    Expect::Main => counted.contains(&PerfWindow::Main),
                    Expect::Windows(windows) => counted.len() >= windows,
                }
            }
            End::Line(text) => watch.wait_for_line(Duration::ZERO, |line| line.contains(text)),
            End::BusName { name, .. } => {
                deskrinch_test_dbus::wait_for(self.bus, name, Duration::ZERO).is_ok()
            }
        }
    }

    /// Waits up to `timeout` for start-up to end, or for something on the
    /// way to it that a run notes when it comes (a window's first frame).
    pub(crate) fn wait(&self, watch: &Watch, timeout: Duration) {
        // Whether what was waited for came is asked again by `ended`.
        let _came: bool = match self.end {
            End::Frame(1) => {
                self.expect.wait(watch, timeout);
                true
            }
            End::Frame(n) => match self.expect {
                Expect::Main => watch.wait_main_frames(*n, timeout),
                Expect::Windows(windows) if self.expect.started(watch) => {
                    watch.wait_frames_each(windows, *n, timeout)
                }
                Expect::Windows(_) => {
                    self.expect.wait(watch, timeout);
                    true
                }
            },
            End::Counter(name) => {
                // Woken by a counted frame of a window not counted yet.
                let counted = counted_windows(watch, name);
                watch.wait_for_line(timeout, |line| {
                    counted_frame(line, name).is_some_and(|window| !counted.contains(&window))
                })
            }
            End::Line(text) => watch.wait_for_line(timeout, |line| line.contains(text)),
            End::BusName { name, .. } => {
                deskrinch_test_dbus::wait_for(self.bus, name, timeout).is_ok()
            }
        };
    }

    /// The error for `program`, watched by `watch`, whose start-up did not
    /// end within `waited`.
    pub(crate) fn not_ended(&self, program: &str, watch: &Watch, waited: Duration) -> Error {
        if self.end.is_first_frame() {
            return windows::no_start(self.expect, program, watch, waited);
        }
        Error::StartNotEnded {
            program: program.to_owned(),
            end: self.end.describe(self.expect),
            waited,
            output: watch.tail(),
        }
    }
}

#[cfg(test)]
mod tests;
