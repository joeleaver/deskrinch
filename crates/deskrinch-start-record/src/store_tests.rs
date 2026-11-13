use std::sync::Arc;

use super::*;
use crate::record::Starts;

const T0: u64 = 1_800_000_000;

fn id(name: &str) -> DesktopId {
    DesktopId::new(format!("{name}.desktop")).unwrap()
}

/// A store in a directory that does not exist yet, under a temporary one.
fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::in_directory(dir.path().join("state/deskrinch"));
    (dir, store)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn read(store: &Store) -> Record {
    match store.load().unwrap() {
        Loaded::Read { record, dropped: 0 } => record,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_record_is_in_the_state_directory() {
    let dirs = BaseDirs::from_vars(|name| (name == "XDG_STATE_HOME").then(|| "/s".into()));
    let store = Store::new(&dirs).unwrap();
    assert_eq!(store.path(), Path::new("/s/deskrinch/starts.toml"));
    assert_eq!(Store::new(&BaseDirs::from_vars(|_| None)), None);
}

#[test]
fn no_file_is_no_record() {
    let (_dir, store) = store();
    assert_eq!(store.load().unwrap(), Loaded::Missing);
}

#[test]
fn a_start_is_written_and_read_back() {
    let (_dir, store) = store();
    let written = store.record(&id("a"), T0).unwrap();
    assert_eq!(written.get(&id("a")), Some(Starts::first(T0)));
    assert_eq!(read(&store), written);
    let again = store.record(&id("a"), T0).unwrap();
    assert_eq!(again.get(&id("a")).unwrap().score, 2.0);
    assert_eq!(read(&store), again);
}

#[test]
fn the_directory_and_the_files_are_the_users_alone() {
    let (dir, store) = store();
    store.record(&id("a"), T0).unwrap();
    assert_eq!(mode(&dir.path().join("state")), 0o700);
    assert_eq!(mode(&dir.path().join("state/deskrinch")), 0o700);
    assert_eq!(mode(&store.path()), 0o600);
    assert_eq!(mode(&dir.path().join("state/deskrinch/starts.lock")), 0o600);
}

#[test]
fn a_file_others_could_read_is_replaced_by_one_they_cannot() {
    let (_dir, store) = store();
    store.record(&id("a"), T0).unwrap();
    fs::set_permissions(store.path(), Permissions::from_mode(0o644)).unwrap();
    store.record(&id("a"), T0).unwrap();
    assert_eq!(mode(&store.path()), 0o600);
}

#[test]
fn a_damaged_file_is_no_record_and_is_replaced_at_the_next_start() {
    for damaged in [&b"not = [toml"[..], b"\xff\xfe\x00", b"version = 0\n"] {
        let (_dir, store) = store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), damaged).unwrap();
        assert!(
            matches!(store.load().unwrap(), Loaded::Damaged { .. }),
            "{damaged:?}"
        );
        assert_eq!(store.load().unwrap().into_record(), Record::new());
        store.record(&id("a"), T0).unwrap();
        assert_eq!(read(&store).len(), 1, "{damaged:?}");
    }
}

#[test]
fn invalid_starts_are_dropped_and_the_rest_kept() {
    let (_dir, store) = store();
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(
        store.path(),
        "version = 1\n[[start]]\nid = \"a.desktop\"\nscore = 3.0\nlast = 5\n\
         [[start]]\nid = \"bad\"\nscore = 1.0\nlast = 5\n",
    )
    .unwrap();
    let Loaded::Read { record, dropped } = store.load().unwrap() else {
        panic!("not read");
    };
    assert_eq!(dropped, 1);
    assert_eq!(record.get(&id("a")).unwrap().score, 3.0);
}

#[test]
fn a_newer_versions_file_is_left_as_it_is() {
    let (_dir, store) = store();
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    let newer = "version = 2\n[[start]]\nid = \"a.desktop\"\n";
    fs::write(store.path(), newer).unwrap();
    assert_eq!(store.load().unwrap(), Loaded::Newer { version: 2 });
    assert!(matches!(
        store.record(&id("b"), T0),
        Err(StoreError::Newer { version: 2, .. })
    ));
    assert_eq!(fs::read_to_string(store.path()).unwrap(), newer);
}

#[test]
fn forgetting_deletes_the_record() {
    let (_dir, store) = store();
    store.record(&id("a"), T0).unwrap();
    assert!(store.forget().unwrap(), "there was a record");
    assert!(!store.path().exists());
    assert_eq!(store.load().unwrap(), Loaded::Missing);
    // Forgotten twice is forgotten.
    assert!(!store.forget().unwrap(), "there was none");
    // The next start begins a new record.
    let record = store.record(&id("b"), T0).unwrap();
    assert_eq!(record.len(), 1);
}

#[test]
fn forgetting_with_no_directory_makes_none() {
    let (dir, store) = store();
    assert!(!store.forget().unwrap());
    assert!(!dir.path().join("state").exists());
}

#[test]
fn a_file_deleted_by_hand_stays_deleted() {
    let (_dir, store) = store();
    store.record(&id("a"), T0).unwrap();
    fs::remove_file(store.path()).unwrap();
    let record = store.record(&id("b"), T0).unwrap();
    assert_eq!(record.get(&id("a")), None);
}

#[test]
fn a_new_file_a_crash_left_behind_is_ignored_then_removed() {
    let (_dir, store) = store();
    store.record(&id("a"), T0).unwrap();
    let directory = store.path().parent().unwrap().to_owned();
    let torn = directory.join(".starts.toml.abc123");
    fs::write(&torn, "version = 1\n[[start]]\nid = \"tor").unwrap();
    assert_eq!(read(&store).len(), 1, "the record is the renamed file");
    store.record(&id("b"), T0).unwrap();
    assert!(!torn.exists());
    let names: Vec<String> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    let mut names = names;
    names.sort();
    assert_eq!(names, ["starts.lock", "starts.toml"]);
}

#[test]
fn concurrent_writers_lose_no_start() {
    const WRITERS: usize = 4;
    const STARTS: usize = 10;
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("deskrinch"));
    let writers: Vec<_> = (0..WRITERS)
        .map(|writer| {
            let path = Arc::clone(&path);
            std::thread::spawn(move || {
                // A store of its own: its own open lock, as another
                // process's would be.
                let store = Store::in_directory(path.as_ref());
                for _ in 0..STARTS {
                    store.record(&id("shared"), T0).unwrap();
                    store.record(&id(&format!("own{writer}")), T0).unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let record = read(&Store::in_directory(path.as_ref()));
    assert_eq!(record.len(), WRITERS + 1);
    #[expect(clippy::cast_precision_loss, reason = "small counts")]
    let all = (WRITERS * STARTS) as f64;
    assert_eq!(record.get(&id("shared")).unwrap().score, all);
    for writer in 0..WRITERS {
        #[expect(clippy::cast_precision_loss, reason = "small counts")]
        let own = STARTS as f64;
        assert_eq!(record.get(&id(&format!("own{writer}"))).unwrap().score, own);
    }
}
