use deskrinch_test_session::ci::Ci;

use super::*;

#[test]
fn refuses_outside_ci() {
    for value in [None, Some(""), Some("false"), Some("1"), Some("TRUE")] {
        let ci = Ci::from_value(value.map(std::ffi::OsStr::new));
        let refused = on_ci(&ci).unwrap_err().to_string();
        assert!(
            refused.starts_with(
                "cargo xtask ci-packages (which installs system packages) runs only on a CI runner"
            ),
            "{value:?}: {refused}"
        );
        assert!(refused.ends_with(&ci.to_string()), "{refused}");
    }
    assert!(on_ci(&Ci::from_value(Some("true".as_ref()))).is_ok());
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
        "-o Acquire::ForceHash=SHA256 -o Dir::Cache::Archives=/tmp/list/archives/ \
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
