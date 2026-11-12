use super::*;

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;
/// Some time well after the epoch.
const T0: u64 = 1_800_000_000;

fn id(name: &str) -> DesktopId {
    DesktopId::new(format!("{name}.desktop")).unwrap()
}

fn ids(record: &Record) -> Vec<String> {
    record
        .ranked()
        .map(|(id, _)| id.app_id().to_owned())
        .collect()
}

fn worth(record: &Record, name: &str, now: u64) -> f64 {
    record.get(&id(name)).unwrap().worth(now)
}

fn near(value: f64, expected: f64) -> bool {
    (value - expected).abs() < 0.05
}

/// A record with `daily` started once a day for thirty days at noon, and
/// `burst` ten times, hourly, on the thirtieth, the last an hour before
/// midnight; answers it and that midnight.
fn daily_and_burst() -> (Record, u64) {
    let mut record = Record::new();
    for day in 0..30 {
        record.record(&id("daily"), T0 + day * DAY + 12 * HOUR);
    }
    let midnight = T0 + 30 * DAY;
    for hour in (1..=10).rev() {
        record.record(&id("burst"), midnight - hour * HOUR);
    }
    (record, midnight)
}

#[test]
fn a_burst_today_leads_a_months_habit_until_the_habit_comes_back() {
    let (mut record, midnight) = daily_and_burst();
    let daily = worth(&record, "daily", midnight);
    let burst = worth(&record, "burst", midnight);
    assert!(near(daily, 9.58), "{daily}");
    assert!(near(burst, 9.77), "{burst}");
    assert_eq!(ids(&record), ["burst", "daily"]);
    // The next day's start, at noon, takes the habit back ahead.
    let noon = midnight + 12 * HOUR;
    record.record(&id("daily"), noon);
    let daily = worth(&record, "daily", noon);
    let burst = worth(&record, "burst", noon);
    assert!(near(daily, 10.12), "{daily}");
    assert!(near(burst, 9.30), "{burst}");
    assert_eq!(ids(&record), ["daily", "burst"]);
}

#[test]
fn starts_halve_every_week() {
    let mut record = Record::new();
    for _ in 0..10 {
        record.record(&id("a"), T0);
    }
    assert!(near(worth(&record, "a", T0), 10.0));
    assert!(near(worth(&record, "a", T0 + 7 * DAY), 5.0));
    assert!(near(worth(&record, "a", T0 + 14 * DAY), 2.5));
}

#[test]
fn an_old_habit_ranks_below_a_single_start_yesterday() {
    let mut record = Record::new();
    for _ in 0..100 {
        record.record(&id("old"), T0);
    }
    let now = T0 + 60 * DAY;
    record.record(&id("new"), now - DAY);
    assert!(
        near(worth(&record, "old", now), 0.26),
        "{}",
        worth(&record, "old", now)
    );
    assert!(
        near(worth(&record, "new", now), 0.91),
        "{}",
        worth(&record, "new", now)
    );
    assert_eq!(ids(&record), ["new", "old"]);
}

#[test]
fn the_ranking_does_not_depend_on_when_it_is_asked() {
    let (record, midnight) = daily_and_burst();
    let order = |now: u64| {
        let mut worths: Vec<(String, f64)> = ["daily", "burst"]
            .iter()
            .map(|name| ((*name).to_owned(), worth(&record, name, now)))
            .collect();
        worths.sort_by(|a, b| b.1.total_cmp(&a.1));
        worths.into_iter().map(|(name, _)| name).collect::<Vec<_>>()
    };
    for later in [0, HOUR, DAY, 30 * DAY, 300 * DAY] {
        assert_eq!(order(midnight + later), ids(&record), "{later}");
    }
}

#[test]
fn equal_starts_rank_by_id() {
    let mut record = Record::new();
    for name in ["c", "a", "b"] {
        record.record(&id(name), T0);
    }
    assert_eq!(ids(&record), ["a", "b", "c"]);
}

#[test]
fn a_start_before_the_last_counts_one_and_no_more() {
    let mut record = Record::new();
    record.record(&id("a"), T0);
    // The clock set back a week.
    record.record(&id("a"), T0 - 7 * DAY);
    let starts = record.get(&id("a")).unwrap();
    assert_eq!(starts.last, T0, "the last start stays where it was");
    assert!(near(starts.score, 2.0), "{}", starts.score);
    // Asked at a time before the last start, the worth is the score.
    assert!(near(starts.worth(T0 - DAY), 2.0));
    // No more than the same two starts made at the same moment.
    let mut same = Record::new();
    same.record(&id("a"), T0);
    same.record(&id("a"), T0);
    assert!(starts.score <= same.get(&id("a")).unwrap().score);
}

#[test]
fn a_single_start_is_forgotten_after_five_weeks() {
    let mut record = Record::new();
    record.record(&id("once"), T0);
    record.prune(T0 + 5 * 7 * DAY - HOUR);
    assert_eq!(record.len(), 1, "still worth more than 1/32");
    record.prune(T0 + 5 * 7 * DAY + HOUR);
    assert_eq!(record, Record::new());
}

#[test]
fn a_start_prunes_the_others() {
    let mut record = Record::new();
    record.record(&id("old"), T0);
    record.record(&id("new"), T0 + 40 * DAY);
    assert_eq!(ids(&record), ["new"]);
}

#[test]
fn only_the_best_are_kept() {
    let mut record = Record::new();
    // Each started at a later time: the later, the better.
    for n in 0..100 {
        record.record(&id(&format!("app{n:03}")), T0 + n);
    }
    assert_eq!(record.len(), MAX_ENTRIES);
    let kept = ids(&record);
    assert_eq!(kept.first().map(String::as_str), Some("app099"));
    assert_eq!(kept.last().map(String::as_str), Some("app036"));
}

#[test]
fn top_takes_the_best_that_are_kept() {
    let mut record = Record::new();
    for (n, name) in ["e", "d", "gone", "c", "b", "a"].iter().enumerate() {
        for _ in 0..=n {
            record.record(&id(name), T0);
        }
    }
    let installed = |id: &DesktopId| id.app_id() != "gone";
    let top: Vec<String> = record
        .top(3, installed)
        .iter()
        .map(|id| id.app_id().to_owned())
        .collect();
    assert_eq!(top, ["a", "b", "c"]);
    assert_eq!(record.top(10, installed).len(), 5);
    assert_eq!(record.top(0, installed), Vec::<DesktopId>::new());
}
