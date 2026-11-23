//! `--shader-cache warm`: the one unmeasured start that fills the cache
//! before the measured runs.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use super::granting::Grant;
use super::start_end::End;
use super::window::Stimulus;
use super::{
    Request, asked, in_session, neighbours, probe, program_client, show, start, with_inputs,
};
use crate::error::Error;

/// Fills the shader cache in `dir` with one start of `request`'s program,
/// unmeasured. It does what a measured run does, its `--window` window and
/// its pointer's steps included, since the cache is to hold what the runs
/// draw; it does not settle, nor wait an idle window.
pub(super) fn fill_cache(
    request: &Request,
    grant: &Grant,
    client: Option<&Path>,
    dir: &Path,
    end: &End,
) -> Result<(), Error> {
    eprintln!("measure: filling the shader cache with one unmeasured start");
    let program = request.program.to_string_lossy().into_owned();
    let timing = probe::Timing {
        first_frame: request.first_frame_timeout,
        settle: Duration::ZERO,
        idle: Duration::ZERO,
    };
    in_session(request, grant, client, |session| {
        with_inputs(request, session, client, |inputs| {
            let command = program_client(session, request, Some(dir));
            match &request.summons {
                Some(summons) => {
                    let ask = |args: &[OsString]| asked(session, request, Some(dir), args);
                    show::measure(
                        command,
                        &ask,
                        inputs,
                        &program,
                        timing,
                        summons,
                        neighbours(session),
                    )
                    .map(drop)
                }
                None => probe::measure(
                    command,
                    &program,
                    timing,
                    &Stimulus::of(session, request.window.as_deref()),
                    start(request, end, session),
                    inputs,
                    None,
                )
                .map(drop),
            }
        })
    })
}
