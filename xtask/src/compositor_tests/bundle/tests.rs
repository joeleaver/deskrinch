use super::*;
use crate::compositor_tests::suites::INPUT_CLIENT;

/// A bundle as the archive leaves it under a fresh root: one test binary,
/// every component, every session test, and xtask at its build path.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    manifest: Manifest,
}

fn executable_file(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let bundle = root.join(DIRECTORY);
        let xtask = PathBuf::from("target/debug/xtask");
        executable_file(&root.join(&xtask), b"xtask");
        let test = PathBuf::from("tests/input_client-1");
        executable_file(&bundle.join(&test), b"test");
        let mut manifest = Manifest {
            root: root.clone(),
            image: None,
            baked: BTreeMap::from([(
                xtask.clone(),
                crate::digest::of_file::<Sha256>(&root.join(&xtask)).unwrap(),
            )]),
            xtask: xtask.clone(),
            tests: BTreeMap::from([(INPUT_CLIENT.name(), test)]),
            granted: BTreeMap::from([(
                INPUT_CLIENT.name(),
                BTreeMap::from([("xtask".to_owned(), xtask)]),
            )]),
            components: BTreeMap::new(),
            session_tests: BTreeMap::new(),
        };
        for package in components::packages() {
            let held = PathBuf::from("components").join(package);
            executable_file(&bundle.join(&held), package.as_bytes());
            manifest.components.insert(package.to_owned(), held);
        }
        for test in SESSION_TESTS {
            let held = PathBuf::from("session-tests").join(test.name);
            executable_file(&bundle.join(&held), test.name.as_bytes());
            manifest.session_tests.insert(test.name.to_owned(), held);
        }
        let fixture = Self {
            _dir: dir,
            root,
            manifest,
        };
        fixture.write();
        fixture
    }

    fn write(&self) {
        let text = serde_json::to_string(&self.manifest).unwrap();
        fs::write(self.root.join(DIRECTORY).join(MANIFEST), text).unwrap();
    }
}

#[test]
fn a_bundle_in_place_opens_and_names_its_binaries() {
    let fixture = Fixture::new();
    let bundle = Bundle::open(&fixture.root).unwrap();
    let dir = fixture.root.join(DIRECTORY);
    assert_eq!(
        bundle.test(INPUT_CLIENT).unwrap(),
        dir.join("tests/input_client-1")
    );
    assert_eq!(
        bundle.granted(INPUT_CLIENT).unwrap(),
        [("xtask", fixture.root.join("target/debug/xtask"))]
    );
    assert_eq!(
        bundle.component("deskrinch-panel").unwrap(),
        dir.join("components/deskrinch-panel")
    );
    assert_eq!(
        bundle.components().unwrap().len(),
        components::packages().len()
    );
    let session = bundle.session_tests().unwrap();
    assert_eq!(session.len(), SESSION_TESTS.len());
    assert_eq!(session[0].0, SESSION_TESTS[0].name);
    let other = Target {
        test: "desktop",
        ..INPUT_CLIENT
    };
    assert!(matches!(
        bundle.test(other),
        Err(BundleError::NotHeld { .. })
    ));
    assert!(bundle.component("deskrinch-nothing").is_err());
}

#[test]
fn a_bundle_built_in_another_root_is_refused() {
    let mut fixture = Fixture::new();
    fixture.manifest.root = "/home/runner/work/deskrinch/deskrinch".into();
    fixture.write();
    let error = Bundle::open(&fixture.root).unwrap_err().to_string();
    assert!(error.contains("CARGO_BIN_EXE_xtask"), "{error}");
}

#[test]
fn an_xtask_other_than_the_one_built_is_refused() {
    let fixture = Fixture::new();
    let xtask = fixture.root.join("target/debug/xtask");
    executable_file(&xtask, b"another unit");
    let error = Bundle::open(&fixture.root).unwrap_err().to_string();
    assert!(
        error.contains("not the file the bundle was built with"),
        "{error}"
    );
    fs::remove_file(&xtask).unwrap();
    assert!(Bundle::open(&fixture.root).is_err(), "nor a missing one");
}

#[test]
fn a_missing_or_unrunnable_binary_is_refused() {
    let fixture = Fixture::new();
    let panel = fixture
        .root
        .join(DIRECTORY)
        .join("components/deskrinch-panel");
    fs::set_permissions(&panel, fs::Permissions::from_mode(0o644)).unwrap();
    let error = Bundle::open(&fixture.root).unwrap_err().to_string();
    assert!(
        error.contains("deskrinch-panel is missing or not executable"),
        "{error}"
    );
    fs::remove_file(&panel).unwrap();
    assert!(Bundle::open(&fixture.root).is_err());
}

#[test]
fn no_bundle_is_an_error_that_says_to_unpack_one() {
    let dir = tempfile::tempdir().unwrap();
    let error = Bundle::open(dir.path()).unwrap_err().to_string();
    assert!(error.contains("unpack the archive"), "{error}");
}
