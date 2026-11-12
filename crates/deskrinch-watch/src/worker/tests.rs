use super::*;
use std::sync::Mutex;
use std::sync::mpsc::RecvTimeoutError;

/// A backend that records watches and refuses any beyond `limit`, as
/// inotify does at `max_user_watches`; its waits return what a test sends.
struct Limited {
    limit: usize,
    watched: Arc<Mutex<Vec<PathBuf>>>,
    waits: Receiver<Wait>,
    wake: Sender<Wait>,
}

impl Backend for Limited {
    fn watch(&mut self, dir: &Path, _how: How) -> Result<(), WatchError> {
        let mut watched = self.watched.lock().unwrap();
        if watched.len() >= self.limit {
            return Err(WatchError::WatchLimit {
                path: Some(dir.to_path_buf()),
            });
        }
        watched.push(dir.to_path_buf());
        Ok(())
    }

    fn unwatch(&mut self, dir: &Path) {
        self.watched.lock().unwrap().retain(|d| d != dir);
    }

    fn wait(&mut self, timeout: Option<Duration>) -> Wait {
        let waited = match timeout {
            Some(timeout) => self.waits.recv_timeout(timeout),
            None => self
                .waits
                .recv()
                .map_err(|_| RecvTimeoutError::Disconnected),
        };
        match waited {
            Ok(wait) => wait,
            Err(RecvTimeoutError::Timeout) => Wait::Quiet,
            Err(RecvTimeoutError::Disconnected) => {
                Wait::Failed(WatchError::from_errno(rustix::io::Errno::BADF, None))
            }
        }
    }

    fn waker(&self) -> Waker {
        let wake = Mutex::new(self.wake.clone());
        Arc::new(move || {
            wake.lock().unwrap().send(Wait::Woken).ok();
        })
    }
}

/// Arms `targets` on a [`Limited`] backend; the sender scripts its waits.
fn arm(limit: usize, targets: Vec<Target>) -> (Result<Armed, WatchError>, Sender<Wait>) {
    let (wake, waits) = mpsc::channel();
    let backend = Limited {
        limit,
        watched: Arc::default(),
        waits,
        wake: wake.clone(),
    };
    let armed = Armed::with_backend(Box::new(backend), targets, Timing::default());
    (armed, wake)
}

#[test]
fn reports_the_watch_limit_when_arming() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("a")).unwrap();
    let targets = vec![Target::File(root.join("a/f")), Target::File(root.join("g"))];
    let (armed, _) = arm(1, targets);
    let error = armed.unwrap_err();
    assert!(
        matches!(&error, WatchError::WatchLimit { path: Some(p) } if p == &root.join("a")),
        "{error:?}"
    );
}

#[test]
fn reports_the_watch_limit_when_the_backend_hits_it_later() {
    let root = tempfile::tempdir().unwrap();
    let (armed, waits) = arm(8, vec![Target::Tree(root.path().to_path_buf())]);
    let (events, received) = mpsc::channel();
    let watcher = armed
        .unwrap()
        .start("test-watch", move |event| {
            events.send(event).ok();
        })
        .unwrap();
    // What inotify reports when a directory made inside a recursive watch
    // cannot be watched.
    waits
        .send(Wait::Changes(vec![Change::Error(WatchError::WatchLimit {
            path: None,
        })]))
        .unwrap();
    let timeout = Duration::from_secs(10);
    assert!(matches!(
        received.recv_timeout(timeout).unwrap(),
        Event::Error(WatchError::WatchLimit { .. })
    ));
    assert!(matches!(
        received.recv_timeout(timeout).unwrap(),
        Event::Changed
    ));
    drop(watcher);
}

#[test]
fn a_failing_backend_is_reported_once_and_the_thread_stops() {
    let root = tempfile::tempdir().unwrap();
    let (armed, waits) = arm(8, vec![Target::Tree(root.path().to_path_buf())]);
    let (events, received) = mpsc::channel();
    let watcher = armed
        .unwrap()
        .start("test-watch", move |event| {
            events.send(event).ok();
        })
        .unwrap();
    waits
        .send(Wait::Failed(WatchError::from_errno(
            rustix::io::Errno::BADF,
            None,
        )))
        .unwrap();
    let timeout = Duration::from_secs(10);
    assert!(matches!(
        received.recv_timeout(timeout).unwrap(),
        Event::Error(WatchError::Backend { .. })
    ));
    // The thread has ended: its subscriber, and with it the sender, is gone.
    assert!(matches!(
        received.recv_timeout(timeout),
        Err(RecvTimeoutError::Disconnected)
    ));
    drop(watcher);
}
