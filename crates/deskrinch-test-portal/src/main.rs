//! `deskrinch-test-portal`: the stand-in settings portal as a program, for
//! checking a component by hand in an isolated session
//! (`cargo xtask session`), never the real one. See
//! [`deskrinch_test_portal::serve_from_env`] for the variables it reads:
//!
//! ```sh
//! echo 2 > scheme; STAND_IN_FILE=scheme deskrinch-test-portal &
//! echo 1 > scheme   # the portal now says dark
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
