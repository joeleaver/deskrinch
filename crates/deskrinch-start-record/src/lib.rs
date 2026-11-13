//! The shell's record of the applications the user starts: how often and
//! how recently, ranked so that what is used most now comes first.
//!
//! # Ranking
//!
//! Each start counts 1 when it is made and halves every [`HALF_LIFE`]
//! (seven days) after. An application's worth is the sum of its starts so
//! decayed, kept as one number and the time of its last start
//! ([`Starts`]): a start decays the number to now and adds one.
//!
//! - An application started once a day for thirty days, last twelve hours
//!   ago, is worth about 9.6; one started ten times today, hourly to an
//!   hour ago, about 9.8, and leads today. At noon tomorrow the second has
//!   decayed to about 9.3 and the first's daily start has taken it to
//!   about 10.1.
//! - Ten starts today and none after are worth 5 in a week, 2.5 in two.
//! - A hundred starts two months ago are worth about 0.26 today: less than
//!   one start yesterday (about 0.9).
//!
//! Two applications decay by the same factor, so which is worth more does
//! not depend on when it is asked: the ranking ([`Record::ranked`]) changes
//! only when a start is recorded, and needs no clock.
//!
//! A start at a time before the application's last one (a clock set back,
//! a file from another machine) counts as made at the last: it adds 1, and
//! never more, since time before the last start is taken as no time.
//!
//! # Bound
//!
//! Each change [prunes](Record::prune) the record: an application worth
//! less than [`FORGOTTEN_BELOW`] (a single start, after five weeks) is
//! forgotten, and only the [`MAX_ENTRIES`] best are kept, so the file stays
//! under a few kilobytes. An application that is no longer installed is not
//! removed: whoever shows the record shows only what is installed, and its
//! starts decay until it is forgotten. (A package upgrade that removes and
//! adds its desktop file again keeps its place.)
//!
//! # The file
//!
//! [`Store`] keeps the record in `$XDG_STATE_HOME/deskrinch/starts.toml`
//! (format: [`VERSION`] 1, one `[[start]]` per application with its desktop
//! entry `id`, `score` and `last`, in seconds since the Unix epoch), the
//! directory the user's alone (0700) and the file too (0600).
//!
//! - **Written whole.** A change writes a new file beside the old, syncs
//!   it, renames it over the old and syncs the directory: a crash leaves
//!   one or the other, never a torn file. A new file a crash left behind is
//!   removed by the next change.
//! - **Several writers.** Every change ([`Store::record`],
//!   [`Store::forget`]) holds an exclusive lock on `starts.lock` beside the
//!   file, reads the file again, changes it and writes it: two writers'
//!   starts are merged, never lost, and a file deleted by hand stays
//!   deleted. Reading ([`Store::load`]) takes no lock.
//! - **A damaged or foreign file** ([`Loaded::Damaged`]) is no record:
//!   whoever loads it starts empty, and says so; the next start writes a
//!   record over it. Invalid starts in a good file are left out, and the
//!   rest are kept.
//! - **A newer version's file** ([`Loaded::Newer`]) is left as it is:
//!   every change is refused ([`StoreError::Newer`]), so an older shell
//!   never rewrites what a newer one wrote.
//!
//! # Who holds it
//!
//! Today the launcher alone holds the record: it loads it at start-up,
//! records the starts it makes and shows the best on an empty query. A
//! second holder (a dock, the panel) would record through the same
//! [`Store`], and stays current by loading the file again
//! ([`Store::load`], one call) whenever it changes: since every change
//! renames a new file into the directory, a watch of the directory
//! (`deskrinch-watch`) sees each one, at no cost while nothing changes.
//! Each holder's own changes are merged with the others' under the lock,
//! so none needs to know of the others.
//!
//! The crate knows no UI: the time is passed in, and the file work is
//! [`Store`]'s alone.

mod format;
mod record;
mod store;

pub use format::VERSION;
pub use record::{FORGOTTEN_BELOW, HALF_LIFE, MAX_ENTRIES, Record, Starts};
pub use store::{DIRECTORY, FILE, LOCK, Loaded, Store, StoreError};

/// The time now, in seconds since the Unix epoch, as the record counts it;
/// 0 for a clock set before the epoch.
#[must_use]
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
