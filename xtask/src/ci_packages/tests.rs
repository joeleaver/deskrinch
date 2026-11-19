use super::*;

#[test]
fn refuses_outside_ci() {
    for value in [None, Some(""), Some("false"), Some("1"), Some("TRUE")] {
        assert!(matches!(on_ci(value), Err(Error::NotInCi)), "{value:?}");
    }
    assert!(on_ci(Some("true")).is_ok());
}

#[test]
fn installs_into_the_cache_with_apts_limits_and_no_recommends() {
    let packages = ["sway".to_owned(), "dbus-daemon".to_owned()];
    let cache = Path::new(cache::DIRECTORY);
    assert_eq!(
        arguments(Step::Install, &packages, cache).join(" "),
        "-o Acquire::Retries=2 -o Acquire::http::Timeout=20 \
         -o Acquire::https::Timeout=20 -o Dir::Cache::Archives=/var/cache/deskrinch-apt/ \
         -o APT::Keep-Downloaded-Packages=true install --yes --no-install-recommends \
         sway dbus-daemon"
    );
    assert_eq!(
        arguments(Step::Update, &packages, cache).join(" "),
        "-o Acquire::Retries=2 -o Acquire::http::Timeout=20 \
         -o Acquire::https::Timeout=20 update"
    );
}

#[test]
fn the_listing_is_the_install_against_an_empty_directory_fetching_nothing() {
    let packages = ["sway".to_owned()];
    assert_eq!(
        arguments(Step::List, &packages, Path::new("/tmp/list")).join(" "),
        "-o Dir::Cache::Archives=/tmp/list/archives/ \
         -o Dir::State::extended_states=/tmp/list/extended_states \
         install --yes --no-install-recommends --print-uris -qq sway"
    );
}

#[test]
fn the_listing_has_an_empty_archives_and_its_own_copy_of_apts_record() {
    let source = tempfile::tempdir().unwrap();
    let record = source.path().join("extended_states");
    std::fs::write(&record, "Package: dbus-user-session\nAuto-Installed: 1\n").unwrap();
    let dir = listing_directory(&record).unwrap();
    assert_eq!(
        std::fs::read_dir(dir.path().join("archives"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("extended_states")).unwrap(),
        "Package: dbus-user-session\nAuto-Installed: 1\n"
    );
    // An image with no record yet: apt starts one of the listing's own.
    let dir = listing_directory(&source.path().join("missing")).unwrap();
    assert!(dir.path().join("archives").is_dir());
    assert!(!dir.path().join("extended_states").exists());
}

#[test]
fn update_and_the_listing_are_bounded_short_and_install_for_a_slow_mirror() {
    assert_eq!(limit(Step::Update), Duration::from_mins(2));
    assert_eq!(limit(Step::List), Duration::from_secs(20));
    assert_eq!(limit(Step::Install), Duration::from_mins(8));
}

#[test]
fn the_outputs_are_appended_as_name_value_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output");
    std::fs::write(&path, "earlier=1\n").unwrap();
    write_outputs(&path, &[("key", "apt-a-b"), ("files", "3")]).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "earlier=1\nkey=apt-a-b\nfiles=3\n"
    );
}

/// What the tightest jobs leave the package step before GitHub ends them:
/// the Tests job and sway's compositor job, a 30 and a 45 minute limit less
/// their other steps at their longest over CI's runs of 2026-11-15 to 18
/// (cold caches included). A verdict is printed only if the step ends
/// within it.
const TIGHTEST_JOB_REMAINDER: Duration = Duration::from_secs(14 * 60 + 42);

/// The time before apt's first line: building xtask, which runs it. At most
/// 44 s over the 220 job logs of 2026-11-17 and 18 that reached apt (median
/// 9 s); the second step's start, with xtask built, is a second or two.
const BUILD_ALLOWANCE: Duration = Duration::from_mins(1);

/// The restore's time besides downloading its one segment (a job's
/// packages are at most 115 MB, under the 128 MB of a segment): looking the
/// key up and unpacking.
const RESTORE_OVERHEAD: Duration = Duration::from_secs(30);

/// The save, on `main` and only when the key was not restored: uploading
/// at most 115 MB, which the action does not bound.
const SAVE_ALLOWANCE: Duration = Duration::from_secs(30);

/// The action's own text, where the restore's limit is set.
const ACTION: &str = include_str!("../../../.github/actions/apt-packages/action.yml");

/// How long the restore may spend on a segment before it gives up (a
/// miss, and the install fetches everything): the action's
/// `SEGMENT_DOWNLOAD_TIMEOUT_MINS`.
fn restore_limit() -> Duration {
    let values: Vec<u64> = ACTION
        .lines()
        .filter_map(|line| line.trim().strip_prefix("SEGMENT_DOWNLOAD_TIMEOUT_MINS:"))
        .map(|value| value.trim().trim_matches('"').parse().unwrap())
        .collect();
    assert_eq!(values.len(), 1, "the action sets it once: {values:?}");
    Duration::from_mins(values[0])
}

/// The step ends, with its verdict printed, and the cache saved, before
/// GitHub ends the tightest job: the build, each bounded run with its grace
/// after `SIGTERM`, the restore and the save.
#[test]
fn the_whole_step_fits_the_tightest_job() {
    let bounded = |limit: Duration| limit + bounded::KILL_AFTER;
    let step = BUILD_ALLOWANCE
        + bounded(cache::PREPARE_LIMIT)
        + bounded(limit(Step::Update))
        + bounded(limit(Step::List))
        + restore_limit()
        + RESTORE_OVERHEAD
        + bounded(limit(Step::Install))
        + SAVE_ALLOWANCE;
    assert!(
        step <= TIGHTEST_JOB_REMAINDER,
        "{step:?} against {TIGHTEST_JOB_REMAINDER:?}"
    );
}
