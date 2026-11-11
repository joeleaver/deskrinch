//! `deskrinch-test-portal`: the stand-in settings portal as a program, for
//! checking a component by hand in an isolated session
//! (`cargo xtask session`), never the real one. See
//! [`deskrinch_test_portal::serve_from_env`] for the variables it reads and
//! [`deskrinch_test_portal::parse`] for the settings file:
//!
//! ```sh
//! echo 'org.freedesktop.appearance color-scheme 2' > settings
//! STAND_IN_FILE=settings deskrinch-test-portal &
//! echo 'org.freedesktop.appearance color-scheme 1' > settings   # now dark
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    match deskrinch_test_portal::serve_from_env(|name| std::env::var_os(name)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("deskrinch-test-portal: {error}");
            ExitCode::FAILURE
        }
    }
}
