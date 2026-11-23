//! Why KWin's answer could not be had.

use std::time::Duration;

use deskrinch_test_session::NotIsolated;

/// Why a question to KWin could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Not inside an isolated session: nothing was asked, and no bus was
    /// touched.
    #[error("refusing to ask KWin: {0}")]
    NotIsolated(NotIsolated),
    /// The address is not a private bus of ours: nothing was asked, and no
    /// bus was touched.
    #[error("refusing to ask KWin: {address} is not a private bus of ours")]
    NotOurBus {
        /// The address refused.
        address: String,
    },
    /// Inside an isolated session, but not KWin's.
    #[error("refusing to ask KWin: the session's compositor is {compositor}, not KWin")]
    NotKwin {
        /// The session's compositor, as it names it.
        compositor: String,
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
    /// KWin's support information has no version line.
    #[error("KWin's support information names no version")]
    NoVersion,
}
