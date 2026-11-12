//! [`Record`]: the starts of each application, decayed with time, and their
//! ranking. Pure: no files and no clock; the time is always passed in.

use std::collections::BTreeMap;

use deskrinch_desktop_entries::DesktopId;

/// The half-life of a start, in seconds: seven days. A start counts 1 when
/// it is made, ½ a week later, ¼ two weeks later.
pub const HALF_LIFE: u64 = 7 * 24 * 60 * 60;

/// How many applications the record keeps at most: the best by rank.
pub const MAX_ENTRIES: usize = 64;

/// The worth below which an application is forgotten: 1/32, five half-lives,
/// so that a single start is forgotten after five weeks.
pub const FORGOTTEN_BELOW: f64 = 1.0 / 32.0;

/// One application's starts: their decayed sum at the last of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Starts {
    /// The decayed number of starts, as it stood at `last`: each start
    /// counted 1 when made and halving every [`HALF_LIFE`] since. Finite
    /// and positive.
    pub score: f64,
    /// When the application was last started, in seconds since the Unix
    /// epoch.
    pub last: u64,
}

impl Starts {
    /// The first start, at `now`.
    #[must_use]
    pub fn first(now: u64) -> Self {
        Self {
            score: 1.0,
            last: now,
        }
    }

    /// What the starts are worth at `now`: the score decayed for the time
    /// since the last start. A `now` before the last start (a clock set
    /// back, a file from another machine) counts as no time at all, so the
    /// worth is never more than the score.
    #[must_use]
    pub fn worth(&self, now: u64) -> f64 {
        self.score * decay(now.saturating_sub(self.last))
    }

    /// One more start, at `now`: the score decayed to `now`, plus one. A
    /// `now` before the last start counts as no time since it, and the last
    /// start stays where it was: the start counts 1, as if it were made at
    /// the last, and never more.
    #[must_use]
    pub fn started(self, now: u64) -> Self {
        Self {
            score: self.worth(now) + 1.0,
            last: self.last.max(now),
        }
    }

    /// The rank key: `log2(score) + last / HALF_LIFE`. Two applications'
    /// worths at any one time compare as their keys do, whatever that time
    /// is, since both decay by the same factor: so the ranking changes only
    /// when a start is recorded.
    fn key(&self) -> f64 {
        // Seconds since the epoch fit an f64 exactly far beyond any clock.
        #[expect(
            clippy::cast_precision_loss,
            reason = "u64 seconds below 2^53 are exact in an f64"
        )]
        let half_lives = self.last as f64 / HALF_LIFE as f64;
        self.score.log2() + half_lives
    }
}

/// The factor a score decays by in `elapsed` seconds.
fn decay(elapsed: u64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "u64 seconds below 2^53 are exact in an f64"
    )]
    let half_lives = elapsed as f64 / HALF_LIFE as f64;
    (-half_lives).exp2()
}

/// The record of starts: each application's [`Starts`], by its desktop
/// entry ID.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    entries: BTreeMap<DesktopId, Starts>,
}

impl Record {
    /// An empty record.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no start is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many applications have starts recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The starts of `id`, if any are recorded.
    #[must_use]
    pub fn get(&self, id: &DesktopId) -> Option<Starts> {
        self.entries.get(id).copied()
    }

    /// Sets the starts of `id`, as read from a file.
    pub(crate) fn insert(&mut self, id: DesktopId, starts: Starts) {
        self.entries.insert(id, starts);
    }

    /// Records a start of `id` at `now`, then [prunes](Self::prune) at
    /// `now`.
    pub fn record(&mut self, id: &DesktopId, now: u64) {
        let starts = match self.entries.get(id) {
            Some(starts) => starts.started(now),
            None => Starts::first(now),
        };
        self.entries.insert(id.clone(), starts);
        self.prune(now);
    }

    /// Forgets the applications worth less than [`FORGOTTEN_BELOW`] at
    /// `now`, then all but the [`MAX_ENTRIES`] best.
    pub fn prune(&mut self, now: u64) {
        self.entries
            .retain(|_, starts| starts.worth(now) >= FORGOTTEN_BELOW);
        if self.entries.len() > MAX_ENTRIES {
            let kept: Vec<DesktopId> = self
                .ranked()
                .take(MAX_ENTRIES)
                .map(|(id, _)| id.clone())
                .collect();
            self.entries.retain(|id, _| kept.contains(id));
        }
    }

    /// Every application and its starts, best first: by worth, which ranks
    /// the same at any time; equal ones by ID.
    pub fn ranked(&self) -> impl Iterator<Item = (&DesktopId, Starts)> {
        let mut ranked: Vec<(&DesktopId, Starts)> =
            self.entries.iter().map(|(id, starts)| (id, *starts)).collect();
        ranked.sort_by(|(a_id, a), (b_id, b)| {
            b.key()
                .total_cmp(&a.key())
                .then_with(|| a_id.cmp(b_id))
        });
        ranked.into_iter()
    }

    /// The `n` best applications that `keep` takes, best first.
    #[must_use]
    pub fn top(&self, n: usize, keep: impl Fn(&DesktopId) -> bool) -> Vec<DesktopId> {
        self.ranked()
            .map(|(id, _)| id)
            .filter(|id| keep(id))
            .take(n)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
