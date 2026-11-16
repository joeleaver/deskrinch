//! The input client from measure's side: `cargo xtask input` run in the
//! session, given one step a line, each acknowledged on its output once
//! taken ([`crate::input`] has the protocol).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::MeasuredInputError;
use crate::error::Error;
use crate::input::{DONE, READY, Step};

/// How long the client may take to make its devices, or to take one step:
/// a guard against a hang, not a time it is expected to take.
const ANSWERS: Duration = Duration::from_secs(60);

/// The input client, running, its devices made.
pub(crate) struct InputClient {
    child: Child,
    /// Its input: a step a line. `None` once closed.
    steps: Option<ChildStdin>,
    /// Its output, a line at a time.
    said: Receiver<String>,
    /// What it writes on standard error, read to its end on a thread, for
    /// a failure's message. `None` once taken.
    stderr: Option<JoinHandle<String>>,
}

impl InputClient {
    /// Starts `command` (`cargo xtask input`, a session client) and waits
    /// for it to say its devices are made.
    pub(crate) fn start(mut command: Command) -> Result<Self, Error> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| MeasuredInputError::Client {
                what: format!("could not be started: {source}"),
                stderr: String::new(),
            })?;
        let steps = child.stdin.take();
        let said = lines(child.stdout.take());
        let stderr = child.stderr.take().map(|mut stream| {
            thread::spawn(move || {
                let mut text = String::new();
                drop(stream.read_to_string(&mut text));
                text
            })
        });
        let mut client = Self {
            child,
            steps,
            said,
            stderr,
        };
        client.expect(READY, "did not make its keyboard and pointer")?;
        Ok(client)
    }

    /// Takes `step`, and waits for the client to say it has.
    pub(crate) fn step(&mut self, step: &Step) -> Result<(), Error> {
        let sent = self
            .steps
            .as_mut()
            .map(|steps| writeln!(steps, "{step}").and_then(|()| steps.flush()));
        if !matches!(sent, Some(Ok(()))) {
            return Err(self.failed(&format!("did not take `{step}`")));
        }
        self.expect(DONE, &format!("did not take `{step}`"))
    }

    /// Closes its input, and waits for it to end as it should.
    pub(crate) fn finish(mut self) -> Result<(), Error> {
        drop(self.steps.take());
        match self.child.wait() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(self.failed(&format!("ended badly ({status})"))),
            Err(source) => Err(self.failed(&format!("could not be waited for: {source}"))),
        }
    }

    /// Waits for the client to say `word`; `failing` says what it did not
    /// do otherwise.
    fn expect(&mut self, word: &str, failing: &str) -> Result<(), Error> {
        match self.said.recv_timeout(ANSWERS) {
            Ok(line) if line == word => Ok(()),
            Ok(line) => Err(self.failed(&format!("{failing}: it said {line:?}"))),
            Err(RecvTimeoutError::Timeout) => {
                Err(self.failed(&format!("{failing} within {ANSWERS:?}")))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(self.failed(&format!("{failing}: it ended")))
            }
        }
    }

    /// The client failed in `what`: it is ended, and what it wrote on its
    /// standard error, to its end, goes with the error.
    fn failed(&mut self, what: &str) -> Error {
        drop(self.steps.take());
        drop(self.child.kill());
        drop(self.child.wait());
        let stderr = self
            .stderr
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        MeasuredInputError::Client {
            what: what.to_owned(),
            stderr,
        }
        .into()
    }
}

impl Drop for InputClient {
    fn drop(&mut self) {
        drop(self.steps.take());
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

/// The lines `stream` writes, as they come, on a thread of their own.
fn lines(stream: Option<impl Read + Send + 'static>) -> Receiver<String> {
    let (send, receive) = mpsc::channel();
    if let Some(stream) = stream {
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
    }
    receive
}
