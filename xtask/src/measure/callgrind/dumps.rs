//! Callgrind's dumps of a profile: asked for through `callgrind_control`,
//! and each figure read from the dump that names it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::Error;

/// Asks Callgrind in process `pid` to write a dump with the hint `label`
/// to the profile `out`, and gives its instruction total ([`totals`]).
pub(crate) fn dump(
    control: &dyn Fn() -> Command,
    pid: &str,
    out: &Path,
    label: &str,
) -> Result<u64, Error> {
    self::control(control, &["--dump", label, pid])?;
    totals(out, label)
}

/// Runs `callgrind_control` (a session client) with `args`: `--dump LABEL
/// PID` writes a dump, `--zero PID` zeroes the counts.
pub(crate) fn control(control: &dyn Fn() -> Command, args: &[&str]) -> Result<(), Error> {
    let status = control()
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| Error::Spawn {
            program: "callgrind_control".to_owned(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CallgrindControl { status })
    }
}

/// The instruction total of the dump of the profile at `out` (Callgrind
/// writes each as `out.N`) whose hint is `label`: the dump that names it in
/// its description (`desc: Trigger: dump LABEL`), whatever its number, so a
/// figure is always read from its own span's dump. Callgrind writes a
/// dump's file when it is asked for it, so the file is waited for.
///
/// A run's hints are unique (`first-frame`, `settle`, `idle`, `show-N`,
/// `hide-N`, `show-N-key-K`, `pointer-K`, `show-N-pointer-K`); two dumps of
/// one profile with the same hint are an error, never a guess.
fn totals(out: &Path, label: &str) -> Result<u64, Error> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let named: Vec<(PathBuf, Option<u64>)> = named_dumps(out)
            .filter(|(_, hint, _)| hint == label)
            .map(|(path, _, total)| (path, total))
            .collect();
        if named.len() > 1 {
            let mut files: Vec<PathBuf> = named.into_iter().map(|(path, _)| path).collect();
            files.sort();
            return Err(Error::CallgrindHintRepeated {
                label: label.to_owned(),
                files,
            });
        }
        if let Some((_, Some(total))) = named.first() {
            return Ok(*total);
        }
        if Instant::now() >= deadline {
            return Err(Error::Read {
                path: out.to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Callgrind wrote no complete dump with the hint {label:?}"),
                ),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The dumps of the profile at `out` written so far: each one's file, its
/// hint, and its total once it is complete.
fn named_dumps(out: &Path) -> impl Iterator<Item = (PathBuf, String, Option<u64>)> {
    let prefix = out
        .file_name()
        .map(|name| format!("{}.", name.to_string_lossy()))
        .unwrap_or_default();
    let dir = out.parent().map(Path::to_owned).unwrap_or_default();
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(move |entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|entry| Some((entry.path(), fs::read_to_string(entry.path()).ok()?)))
        .filter_map(|(path, text)| Some((path, hint(&text)?.to_owned(), parse_totals(&text))))
}

/// The hint a dump was asked for with, from its description.
fn hint(profile: &str) -> Option<&str> {
    profile
        .lines()
        .find_map(|line| line.strip_prefix("desc: Trigger: dump "))
        .map(str::trim_end)
}

/// The `totals:` (or, without it, `summary:`) line of a Callgrind profile:
/// its instruction count.
fn parse_totals(profile: &str) -> Option<u64> {
    let find = |key: &str| {
        profile.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    find("totals:").or_else(|| find("summary:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_total_of_a_dump() {
        let profile = "version: 1\ncreator: callgrind-3.26.0\npid: 1309\n\
                       events: Ir\nfn=(1) main\n5 100\nsummary: 2310257585\n\
                       totals: 2310257585\n";
        assert_eq!(parse_totals(profile), Some(2_310_257_585));
        assert_eq!(parse_totals("events: Ir\nsummary: 358\n"), Some(358));
        assert_eq!(parse_totals("events: Ir\n"), None);
    }

    /// A dump with the hint `hint` and the total `total`, as Callgrind
    /// writes one.
    fn dump_text(hint: &str, total: u64) -> String {
        format!(
            "version: 1\ncreator: callgrind-3.26.0\npart: 2\ndesc: I1 cache: \n\
             desc: Trigger: dump {hint}\n\npositions: line\nevents: Ir\nsummary: {total}\n\
             totals: {total}\n"
        )
    }

    #[test]
    fn a_figure_is_read_from_the_dump_that_names_it_whatever_its_number() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("callgrind.out");
        fs::write(dir.path().join("callgrind.out.1"), dump_text("idle", 900)).unwrap();
        fs::write(
            dir.path().join("callgrind.out.3"),
            dump_text("pointer-2", 70),
        )
        .unwrap();
        fs::write(
            dir.path().join("callgrind.out.2"),
            dump_text("pointer-1", 50),
        )
        .unwrap();
        fs::write(dir.path().join("other.out.2"), dump_text("pointer-1", 1)).unwrap();
        assert_eq!(totals(&out, "pointer-1").unwrap(), 50);
        assert_eq!(totals(&out, "pointer-2").unwrap(), 70);
        assert_eq!(totals(&out, "idle").unwrap(), 900);
        assert_eq!(
            hint(&dump_text("show-1-pointer-1", 5)),
            Some("show-1-pointer-1")
        );
        assert_eq!(
            hint("version: 1\ndesc: Trigger: Program termination\n"),
            None
        );
    }

    #[test]
    fn two_dumps_of_one_profile_with_one_hint_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("callgrind.out");
        fs::write(
            dir.path().join("callgrind.out.1"),
            dump_text("pointer-1", 50),
        )
        .unwrap();
        fs::write(
            dir.path().join("callgrind.out.2"),
            dump_text("pointer-1", 70),
        )
        .unwrap();
        let error = totals(&out, "pointer-1").unwrap_err().to_string();
        assert!(error.contains("\"pointer-1\""), "{error}");
        assert!(
            error.contains("callgrind.out.1") && error.contains("callgrind.out.2"),
            "{error}"
        );
    }
}
