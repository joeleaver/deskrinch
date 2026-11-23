//! The rule that a compositor test starts a shell component only by the path
//! the runner hands it ([`deskrinch_test_component::programs`], where the
//! rule is given), kept by a check rather than by memory: no suite's
//! sources may name a component's `CARGO_BIN_EXE_<package>`, the binary the
//! tests' own build linked with the test crates' features.
//!
//! The check reads each suite target's own files: `tests/<test>.rs`, or every
//! file under `tests/<test>/`, and the files a `#[path = "..."]` in them
//! points to (a support module shared between targets), following those in
//! turn. What a target reaches some other way (a library crate's code) is
//! not a test's and not scanned.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use deskrinch_shell::{COMPONENTS, LAUNCHER_TOGGLE};

    use super::super::build::Target;
    use super::super::suites::SUITES;

    /// The workspace's root, above xtask's manifest.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask is in the workspace")
            .to_owned()
    }

    /// The directory of `package`.
    fn package_dir(package: &str) -> PathBuf {
        if package == "xtask" {
            root().join("xtask")
        } else {
            root().join("crates").join(package)
        }
    }

    /// Every `.rs` file under `dir`.
    fn rust_files(dir: &Path, found: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.insert(path);
            }
        }
    }

    /// The files `#[path = "..."]` in `text`, a file in `dir`, points to:
    /// each file, or for a `mod.rs` its whole directory.
    fn path_attributes(text: &str, dir: &Path, found: &mut BTreeSet<PathBuf>) -> Vec<PathBuf> {
        let mut new = Vec::new();
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("#[path") else {
                continue;
            };
            let Some(quoted) = rest.split('"').nth(1) else {
                continue;
            };
            // `../support/mod.rs` from `tests/compositor`: the file itself.
            let target = fs::canonicalize(dir.join(quoted)).unwrap_or_else(|_| dir.join(quoted));
            let mut reached = BTreeSet::new();
            if target.file_name().is_some_and(|name| name == "mod.rs")
                && let Some(module) = target.parent()
            {
                rust_files(module, &mut reached);
            } else {
                reached.insert(target);
            }
            for file in reached {
                if found.insert(file.clone()) {
                    new.push(file);
                }
            }
        }
        new
    }

    /// The files of `target`: its own, and those `#[path]` reaches.
    fn sources(target: Target) -> BTreeSet<PathBuf> {
        let tests = package_dir(target.package).join("tests");
        let mut found = BTreeSet::new();
        let single = tests.join(format!("{}.rs", target.test));
        if single.is_file() {
            found.insert(single);
        } else {
            rust_files(&tests.join(target.test), &mut found);
        }
        let mut pending: Vec<PathBuf> = found.iter().cloned().collect();
        while let Some(file) = pending.pop() {
            let text = fs::read_to_string(&file).unwrap_or_default();
            let dir = file.parent().unwrap_or(&tests).to_owned();
            pending.extend(path_attributes(&text, &dir, &mut found));
        }
        found
    }

    /// The packages whose binaries are the shell's components.
    fn components() -> BTreeSet<&'static str> {
        COMPONENTS
            .iter()
            .map(|component| component.package)
            .chain([LAUNCHER_TOGGLE.package])
            .collect()
    }

    /// What breaks the rule in `text` (the file `path`): each line naming a
    /// component's `CARGO_BIN_EXE_`, with what to do instead.
    fn breaches(path: &Path, text: &str, components: &BTreeSet<&'static str>) -> Vec<String> {
        let mut found = Vec::new();
        for (number, line) in text.lines().enumerate() {
            for package in components {
                if line.contains(&format!("CARGO_BIN_EXE_{package}")) {
                    found.push(format!(
                        "{}:{}: names `CARGO_BIN_EXE_{package}`, the binary the tests' own build \
                         linked; start {package} by the path the runner hands the suite, \
                         `deskrinch_test_component::programs::program(\"{package}\")`, with the \
                         suite declaring it in its `programs` (xtask/src/compositor_tests/\
                         suites/)",
                        path.display(),
                        number + 1
                    ));
                }
            }
        }
        found
    }

    #[test]
    fn no_compositor_test_starts_a_component_by_its_cargo_bin_exe() {
        let components = components();
        let mut found = Vec::new();
        let mut scanned = BTreeSet::new();
        for suite in &SUITES {
            for file in sources(suite.target) {
                if scanned.insert(file.clone()) {
                    let text = fs::read_to_string(&file).unwrap_or_default();
                    found.extend(breaches(&file, &text, &components));
                }
            }
        }
        assert!(
            scanned.len() > SUITES.len(),
            "the suites' sources were found: {scanned:?}"
        );
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    #[test]
    fn a_shared_support_module_is_scanned_through_its_path() {
        let target = Target {
            package: "deskrinch-test-screen",
            test: "compositor",
            backend: super::super::build::Backend::Rust,
        };
        let sources = sources(target);
        assert!(
            sources
                .iter()
                .any(|file| file.ends_with("tests/support/mod.rs")),
            "{sources:?}"
        );
    }

    #[test]
    fn a_breach_says_what_to_do_instead() {
        let components = components();
        let text = "const X: &str = env!(\"CARGO_BIN_EXE_deskrinch-panel\");\n\
                    const Y: &str = env!(\"CARGO_BIN_EXE_xtask\");\n";
        let found = breaches(Path::new("tests/compositor/support.rs"), text, &components);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("tests/compositor/support.rs:1: "),
            "{}",
            found[0]
        );
        assert!(
            found[0].contains("programs::program(\"deskrinch-panel\")"),
            "{}",
            found[0]
        );
        assert!(found[0].contains("its `programs`"), "{}", found[0]);
    }
}
