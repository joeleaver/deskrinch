//! The typist from measure's side: `cargo xtask type` run in the session,
//! given one key at a time on its input, each acknowledged on its output
//! once typed ([`crate::typist`]).

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::TypingError;
use crate::error::Error;
use crate::typist::{READY, TYPED};

/// How long the typist may take to make its keyboard, or to type one key:
/// a guard against a hang, not a time it is expected to take.
const ANSWERS: Duration = Duration::from_secs(60);

/// The typist, running, its keyboard made.
pub(crate) struct Typist {
    child: Child,
    /// Its input: a key a line. `None` once closed.
    keys: Option<ChildStdin>,
    /// Its output, a line at a time.
    said: Receiver<String>,
    /// What it wrote on standard error, for a failure's message.
    stderr: Arc<Mutex<String>>,
}

impl Typist {
    /// Starts `command` (`cargo xtask type`, a session client) and waits
    /// for it to say its keyboard is made.
    pub(crate) fn start(mut command: Command) -> Result<Self, Error> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| TypingError::Typist {
                what: format!("could not be started: {source}"),
                stderr: String::new(),
            })?;
        let keys = child.stdin.take();
        let said = lines(child.stdout.take());
        let stderr = Arc::new(Mutex::new(String::new()));
        if let Some(mut stream) = child.stderr.take() {
            let stderr = Arc::clone(&stderr);
            thread::spawn(move || {
                let mut text = String::new();
                drop(stream.read_to_string(&mut text));
                stderr
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_str(&text);
            });
        }
        let mut typist = Self {
            child,
            keys,
            said,
            stderr,
        };
        typist.expect(READY, "did not make its keyboard")?;
        Ok(typist)
    }

    /// Types `key`, and waits for the typist to say it has.
    pub(crate) fn key(&mut self, key: char) -> Result<(), Error> {
        let sent = self
            .keys
            .as_mut()
            .map(|keys| writeln!(keys, "{key}").and_then(|()| keys.flush()));
        if !matches!(sent, Some(Ok(()))) {
            return Err(self.failed(&format!("did not take the key {key:?}")));
        }
        self.expect(TYPED, &format!("did not type the key {key:?}"))
    }

    /// Closes its input, and waits for it to end as it should.
    pub(crate) fn finish(mut self) -> Result<(), Error> {
        drop(self.keys.take());
        match self.child.wait() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(self.failed(&format!("ended badly ({status})"))),
            Err(source) => Err(self.failed(&format!("could not be waited for: {source}"))),
        }
    }

    /// Waits for the typist to say `word`; `failing` says what it did not
    /// do otherwise.
    fn expect(&mut self, word: &str, failing: &str) -> Result<(), Error> {
        match self.said.recv_timeout(ANSWERS) {
            Ok(line) if line == word => Ok(()),
            Ok(line) => Err(self.failed(&format!("{failing}: it said {line:?}"))),
            Err(RecvTimeoutError::Timeout) => {
                Err(self.failed(&format!("{failing} within {ANSWERS:?}")))
            }
            Err(RecvTimeoutError::Disconnected) => {
                drop(self.child.wait());
                Err(self.failed(&format!("{failing}: it ended")))
            }
        }
    }

    fn failed(&self, what: &str) -> Error {
        TypingError::Typist {
            what: what.to_owned(),
            stderr: self
                .stderr
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
        .into()
    }
}

impl Drop for Typist {
    fn drop(&mut self) {
        drop(self.keys.take());
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
