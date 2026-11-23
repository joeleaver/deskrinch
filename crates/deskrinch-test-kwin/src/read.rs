//! Reading every window: the object the script reports to, and the wait for
//! its final call.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use deskrinch_test_wait::PATIENCE;

use crate::script::{self, INTERFACE, Loaded};
use crate::{Error, Window, guard};

/// What the script said.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Report {
    /// A window's record.
    Window(String),
    /// Its final call: how many windows it reported.
    Done(String),
}

/// The object the script calls, under a name this process owns.
struct Reports {
    to: Sender<Report>,
}

#[zbus::interface(name = "org.deskrinch.TestKwin")]
impl Reports {
    /// A window's record.
    fn window(&self, record: String) {
        // The reader may have given up; its error says so already.
        drop(self.to.send(Report::Window(record)));
    }

    /// The final call.
    fn done(&self, count: String) {
        drop(self.to.send(Report::Done(count)));
    }
}

/// Every window KWin has, as KWin holds it, in the order KWin lists them.
///
/// # Errors
///
/// [`Error::NotIsolated`] or [`Error::NotKwin`] outside an isolated KWin
/// session, before any connection; [`Error::Bus`] or [`Error::NotLoaded`]
/// when the script cannot be run; [`Error::NoFinalCall`] when it does not
/// finish its report within [`PATIENCE`]; [`Error::Miscounted`] or
/// [`Error::Record`] when its report is not one this crate reads.
pub fn windows() -> Result<Vec<Window>, Error> {
    guard::guarded(guard::require_kwin_here(), ask)
}

/// [`windows`], once the session's check has passed.
fn ask() -> Result<Vec<Window>, Error> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let service = format!("{INTERFACE}.p{pid}.n{call}");
    let plugin = format!("deskrinch-test-kwin-{pid}-{call}");
    let (to, from) = channel();
    let records = zbus::block_on(async {
        let connection = zbus::connection::Builder::session()
            .name(service.as_str())
            .serve_at("/", Reports { to })
            .build()
            .await?;
        Loaded::run(&connection, &plugin, &script::source(&service)).await
    })
    .and_then(|loaded| {
        let records = collect(&from, PATIENCE);
        drop(loaded);
        records
    })?;
    records
        .iter()
        .map(|record| Window::from_record(record))
        .collect()
}

/// The records `reports` brings, up to the final call, which must come
/// within `limit` and name their count.
fn collect(reports: &Receiver<Report>, limit: Duration) -> Result<Vec<String>, Error> {
    let start = Instant::now();
    let mut records = Vec::new();
    loop {
        let left = limit.saturating_sub(start.elapsed());
        match reports.recv_timeout(left) {
            Ok(Report::Window(record)) => records.push(record),
            Ok(Report::Done(said)) => {
                return if said == records.len().to_string() {
                    Ok(records)
                } else {
                    Err(Error::Miscounted {
                        arrived: records.len(),
                        said,
                    })
                };
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                return Err(Error::NoFinalCall {
                    waited: limit,
                    arrived: records.len(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use zbus::object_server::Interface as _;

    use super::*;

    #[test]
    fn the_object_serves_the_interface_the_script_calls() {
        assert_eq!(Reports::name().as_str(), INTERFACE);
    }

    #[test]
    fn the_records_up_to_the_final_call_are_the_answer() {
        let (to, from) = channel();
        to.send(Report::Window("a".into())).unwrap();
        to.send(Report::Window("b".into())).unwrap();
        to.send(Report::Done("2".into())).unwrap();
        assert_eq!(collect(&from, Duration::from_secs(5)).unwrap(), ["a", "b"]);
    }

    /// A script that throws reaches no final call: the limit is the only
    /// sign, and the error says how far it got.
    #[test]
    fn no_final_call_within_the_limit_says_how_many_had_arrived() {
        let (to, from) = channel();
        to.send(Report::Window("a".into())).unwrap();
        let refusal = collect(&from, Duration::from_millis(50)).unwrap_err();
        assert!(
            matches!(refusal, Error::NoFinalCall { arrived: 1, .. }),
            "{refusal:?}"
        );
        assert!(
            refusal
                .to_string()
                .contains("(1 windows had arrived): it may have thrown"),
            "{refusal}"
        );
        drop(to);
    }

    #[test]
    fn a_final_call_with_another_count_is_an_error() {
        let (to, from) = channel();
        to.send(Report::Window("a".into())).unwrap();
        to.send(Report::Done("3".into())).unwrap();
        assert!(matches!(
            collect(&from, Duration::from_secs(5)),
            Err(Error::Miscounted { arrived: 1, said }) if said == "3"
        ));
    }
}
