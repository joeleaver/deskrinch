//! What a shard checks of the bundle before it runs anything: the runner
//! image it was built on against this one (said, never failed on), and
//! every library its binaries need (failed on when one is missing).

use std::path::PathBuf;
use std::process::Command;

use super::{BundleError, Image};
use crate::error::Error;

/// What a shard says of the image the bundle was built on (`built`) and
/// the one it runs on (`here`). A difference is a notice, not a failure:
/// within one release every library keeps its interface, and
/// [`libraries`] catches one that is missing.
pub(super) fn images(built: Option<&Image>, here: Option<Image>) -> String {
    match (built, here) {
        (Some(built), Some(here)) if *built == here => {
            format!("bundle: built on this runner image, {here}")
        }
        (Some(built), Some(here)) => format!(
            "::notice title=Runner images differ::the bundle was built on {built} and runs on \
             {here}"
        ),
        (None, Some(here)) => format!("bundle: built off a runner; this is {here}"),
        (Some(built), None) => format!("bundle: built on {built}; this is no runner"),
        (None, None) => "bundle: built and run off a runner".to_owned(),
    }
}

/// Fails, listing them, when a library one of `files` needs is not on
/// this machine, as `ldd` finds them.
pub(super) fn libraries(files: &[PathBuf]) -> Result<(), Error> {
    let output = Command::new("ldd")
        .args(files)
        .output()
        .map_err(|source| Error::Spawn {
            program: "ldd".into(),
            source,
        })?;
    let lines = missing(&String::from_utf8_lossy(&output.stdout));
    if lines.is_empty() {
        Ok(())
    } else {
        Err(BundleError::MissingLibraries { lines }.into())
    }
}

/// The lines of `ldd`'s `output` (over several files, each under its own
/// `path:` line) that name a library not found, each with its file.
fn missing(output: &str) -> Vec<String> {
    let mut file = "";
    let mut lines = Vec::new();
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            file = line.trim_end_matches(':');
        } else if line.contains("=> not found") {
            lines.push(format!("{file}: {}", line.trim()));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(version: &str) -> Image {
        Image {
            os: "ubuntu26".into(),
            version: version.into(),
        }
    }

    #[test]
    fn a_different_image_is_a_notice() {
        let said = images(Some(&image("1")), Some(image("2")));
        assert!(said.starts_with("::notice"), "{said}");
        assert!(said.contains("ubuntu26 1 and runs on ubuntu26 2"), "{said}");
        assert_eq!(
            images(Some(&image("1")), Some(image("1"))),
            "bundle: built on this runner image, ubuntu26 1"
        );
        assert!(!images(None, None).contains("notice"));
    }

    /// As `ldd` prints it for two files, one lacking a library.
    const LDD: &str = "/b/tests/compositor-1:\n\
        \tlinux-vdso.so.1 (0x00007ffd)\n\
        \tlibxdo.so.3 => not found\n\
        \tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f)\n\
        /b/components/deskrinch-panel:\n\
        \tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f)\n";

    #[test]
    fn a_library_not_found_is_named_with_its_file() {
        assert_eq!(
            missing(LDD),
            ["/b/tests/compositor-1: libxdo.so.3 => not found"]
        );
        assert_eq!(
            missing("/b/x:\n\tlibc.so.6 => /lib/libc.so.6 (0x1)\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn this_test_binary_has_every_library() {
        let me = std::env::current_exe().unwrap();
        libraries(&[me]).unwrap();
    }
}
