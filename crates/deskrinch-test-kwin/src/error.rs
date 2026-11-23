//! Why KWin's answer could not be had.

use std::fmt;
use std::time::Duration;

use deskrinch_test_session::NotIsolated;

/// Why a question to KWin, or a change in it, could not be had.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A handle was refused before any connection: nothing was asked or
    /// changed, and no bus was touched.
    #[error("refusing to {act} KWin: {why}")]
    Refused {
        /// What the handle was for.
        act: Act,
        /// Why it was refused.
        why: Refusal,
    },
    /// The session's bus could not be used.
    #[error("the session's bus: {0}")]
    Bus(#[from] zbus::Error),
    /// KWin's scripting interface answered, but would not load the script.
    #[error("KWin would not load the script {plugin} (loadScript answered {id})")]
    NotLoaded {
        /// The plugin name it was loaded under.
        plugin: String,
        /// What `loadScript` answered.
        id: i32,
    },
    /// The script's file could not be written.
    #[error("could not write the script's file: {0}")]
    File(#[from] std::io::Error),
    /// The script did not make its final call in time. KWin tells the caller
    /// nothing of a script that throws (it logs it): this limit is the only
    /// sign of one.
    #[error(
        "the script made no final call within {} s ({arrived} windows had arrived): it may have \
         thrown, which KWin tells only its own log",
        waited.as_secs()
    )]
    NoFinalCall {
        /// How long it was waited for.
        waited: Duration,
        /// How many windows it had reported by then.
        arrived: usize,
    },
    /// The script's final call named another count than the windows it had
    /// reported.
    #[error("the script reported {arrived} windows and then said there were {said}")]
    Miscounted {
        /// How many it reported.
        arrived: usize,
        /// How many its final call said.
        said: String,
    },
    /// A window's record is not one this crate reads.
    #[error("a window's record is not one this crate reads ({reason}): {record}")]
    Record {
        /// The record, as the script wrote it.
        record: String,
        /// What is wrong with it.
        reason: String,
    },
    /// An answer is not of the shape this crate reads.
    #[error("KWin's answer is not one this crate reads ({reason}): {answer}")]
    Answer {
        /// The answer, as zbus shows it.
        answer: String,
        /// What is wrong with it.
        reason: String,
    },
    /// KWin still lists fewer virtual desktops than asked for after as many
    /// creations as were asked for: it ignored or capped them.
    #[error("asked for {asked} virtual desktops; after {asked} creations KWin lists {listed}")]
    TooFewDesktops {
        /// How many were asked for.
        asked: usize,
        /// How many KWin lists.
        listed: usize,
    },
    /// KWin lists no virtual desktop at all.
    #[error("KWin lists no virtual desktop")]
    NoDesktops,
    /// KWin has no virtual desktop of that name.
    #[error("KWin has no virtual desktop named {name}")]
    NoSuchDesktop {
        /// The name asked for.
        name: String,
    },
    /// KWin's current virtual desktop is not among those it lists.
    #[error("KWin's current virtual desktop {desktop} is not among those it lists")]
    NotListed {
        /// Its id.
        desktop: String,
    },
    /// KWin's support information has no version line.
    #[error("KWin's support information names no version")]
    NoVersion,
}

/// What a refused handle was for: asking ([`Kwin`](crate::Kwin)) or changing
/// ([`KwinActions`](crate::KwinActions)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Asking KWin.
    Ask,
    /// Changing something in KWin.
    Change,
}

impl fmt::Display for Act {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ask => "ask",
            Self::Change => "change",
        })
    }
}

/// Why a handle was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The process is not inside an isolated session of ours.
    NotIsolated(NotIsolated),
    /// The address is not a private bus of ours.
    NotOurBus {
        /// The address refused.
        address: String,
    },
    /// Inside an isolated session, but not KWin's.
    NotKwin {
        /// The session's compositor, as it names it.
        compositor: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIsolated(why) => why.fmt(f),
            Self::NotOurBus { address } => write!(f, "{address} is not a private bus of ours"),
            Self::NotKwin { compositor } => {
                write!(f, "the session's compositor is {compositor}, not KWin")
            }
        }
    }
}

impl Error {
    /// The same error, a refusal made for a change rather than a question.
    pub(crate) fn for_a_change(self) -> Self {
        match self {
            Self::Refused { why, .. } => Self::Refused {
                act: Act::Change,
                why,
            },
            other => other,
        }
    }
}
