//! `--shader-cache warm`: the cache filled by one unmeasured start before
//! the measured runs, kept outside the sessions for every run.
//!
//! The filling start does what a measured run does, its `--window` window,
//! its pointer's steps, its setup commands and its command steps included:
//! the cache is to hold what the runs draw. A daemon that draws nothing
//! until a step (a notification shown) would otherwise fill nothing, and
//! the first measured run would pay for the cache.

use std::path::Path;
use std::time::Duration;

use super::granting::Grant;
use super::scripted::Scripted;
use super::start_end::End;
use super::window::Stimulus;
use super::{Request, asked, in_session, probe, program_client, show, start, with_inputs};
use crate::error::Error;

/// Fills the shader cache in `dir` with one unmeasured start of `program`,
/// as `request` asks it measured.
pub(super) fn fill(
    request: &Request,
    grant: &Grant,
    client: Option<&Path>,
    end: &End,
    program: &str,
    dir: &Path,
) -> Result<(), Error> {
    eprintln!("measure: filling the shader cache with one unmeasured start");
    let timing = probe::Timing {
        first_frame: request.first_frame_timeout,
        settle: Duration::ZERO,
        idle: Duration::ZERO,
    };
    in_session(request, grant, client, |session| {
        with_inputs(request, session, client, |inputs| {
            let command = program_client(session, request, Some(dir));
            if let Some(summons) = &request.summons {
                let ask = |args: &[std::ffi::OsString]| asked(session, request, Some(dir), args);
                return show::measure(command, &ask, inputs, program, timing, summons).map(drop);
            }
            let shell = || session.client("sh".as_ref());
            probe::measure(
                command,
                program,
                timing,
                &Stimulus::of(session, request.window.as_deref()),
                start(request, end, session),
                inputs,
                &Scripted::of(request, &shell),
            )
            .map(drop)
        })
    })
}
