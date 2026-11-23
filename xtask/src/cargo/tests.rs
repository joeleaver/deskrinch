use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Stdio;

use super::*;

/// A `RUSTC_WRAPPER` that lets cargo's own questions to rustc through
/// (`-vV`, and the target probe named `___`), and at the first real compile
/// writes where rustc runs and which compiler answers there to the file
/// `DESKRINCH_WRAPPER_REPORT` names, then fails: nothing is compiled.
const WRAPPER: &str = r#"#!/bin/sh
case " $* " in
  *" -vV "*|*" --crate-name ___ "*) exec "$@" ;;
esac
{ echo "dir=$(pwd)"; echo "version=$("$1" --version)"; } > "$DESKRINCH_WRAPPER_REPORT"
exit 1
"#;

/// The toolchain `rust-toolchain.toml` pins: its `channel`.
fn pinned() -> String {
    let text =
        fs::read_to_string(Path::new(crate::WORKSPACE_ROOT).join("rust-toolchain.toml")).unwrap();
    text.lines()
        .find_map(|line| line.strip_prefix("channel = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap()
        .to_owned()
}

#[test]
fn the_cargo_named_by_the_environment_or_else_the_proxy() {
    assert_eq!(program(Some("/a/cargo".into())), "/a/cargo");
    assert_eq!(program(None), "cargo");
}

/// From a state with neither `RUSTUP_TOOLCHAIN` nor `CARGO` (a process
/// tree no cargo started), a registry crate is compiled by the pinned
/// compiler. The wrapper compiles nothing, in target and build directories
/// of the test's own.
#[test]
fn a_registry_crate_is_compiled_by_the_pinned_compiler() {
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("rustc-wrapper");
    fs::write(&wrapper, WRAPPER).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let report = dir.path().join("report");
    let status = Command::new(program(None))
        .current_dir(crate::WORKSPACE_ROOT)
        .args([
            "check",
            "--offline",
            "--locked",
            "--jobs",
            "1",
            "--package",
            "deskrinch-test-dbus",
        ])
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO")
        .env("RUSTC_WRAPPER", &wrapper)
        .env("DESKRINCH_WRAPPER_REPORT", &report)
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .env("CARGO_BUILD_BUILD_DIR", dir.path().join("build"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success(), "the wrapper let a compile through");
    let report = fs::read_to_string(&report).unwrap();
    let field = |name: &str| {
        report
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("no {name} in {report:?}"))
            .to_owned()
    };
    assert!(field("dir").contains("/registry/src/"), "{report}");
    let version = field("version");
    assert!(
        version.starts_with(&format!("rustc {} ", pinned())),
        "{version}, not the pinned {}",
        pinned()
    );
}
