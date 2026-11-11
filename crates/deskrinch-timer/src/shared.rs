//! The deadlines a timer's thread keeps, and the thread itself.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::Waker;
use std::thread;
use std::time::Instant;

/// What the timer's handles, futures and thread share.
pub(crate) struct Shared {
    state: Mutex<State>,
    /// Wakes the thread when a deadline is added or the last user leaves.
    changed: Condvar,
}

#[derive(Default)]
struct State {
    /// The deadlines of the live futures not yet reached, earliest first.
    heap: BinaryHeap<Reverse<(Instant, u64)>>,
    /// A slot for each deadline in `heap`, by key, holding its future's
    /// waker (`None` until it is polled). A slot exists exactly while its
    /// deadline is in the heap.
    wakers: HashMap<u64, Option<Waker>>,
    next_key: u64,
    /// Handles and futures alive; the thread ends when none is.
    users: usize,
}

impl Shared {
    /// The state for one handle, and its thread.
    pub(crate) fn start() -> io::Result<Arc<Self>> {
        let shared = Arc::new(Self {
            state: Mutex::new(State {
                users: 1,
                ..State::default()
            }),
            changed: Condvar::new(),
        });
        let thread_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("deskrinch-timer".into())
            .spawn(move || thread_shared.run())?;
        Ok(shared)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Another handle.
    pub(crate) fn join(&self) {
        self.lock().users += 1;
    }

    /// A handle is gone.
    pub(crate) fn leave(&self) {
        let mut state = self.lock();
        state.users -= 1;
        if state.users == 0 {
            self.changed.notify_all();
        }
    }

    /// A new future's `deadline`; its key.
    pub(crate) fn insert(&self, deadline: Instant) -> u64 {
        let mut state = self.lock();
        let key = state.next_key;
        state.next_key += 1;
        state.users += 1;
        state.heap.push(Reverse((deadline, key)));
        state.wakers.insert(key, None);
        self.changed.notify_all();
        key
    }

    /// Future `key` was polled with `waker`: `true` if its deadline is still
    /// to come, with `waker` kept to wake it; `false` if the thread has
    /// already taken the deadline, with nothing kept. Deciding under the
    /// lock that keeps the waker is what makes a wake-up impossible to lose.
    pub(crate) fn set_waker(&self, key: u64, waker: &Waker) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.wakers.get_mut(&key) else {
            return false;
        };
        match slot {
            Some(old) if old.will_wake(waker) => {}
            _ => *slot = Some(waker.clone()),
        }
        true
    }

    /// Future `key` is gone: its deadline and waker go with it.
    pub(crate) fn remove(&self, key: u64) {
        let mut state = self.lock();
        if state.wakers.remove(&key).is_some() {
            state.heap.retain(|&Reverse((_, k))| k != key);
        }
        state.users -= 1;
        if state.users == 0 {
            self.changed.notify_all();
        }
    }

    /// The thread: wakes each future whose deadline has passed, in the
    /// order of their deadlines, until nobody uses the timer.
    fn run(&self) {
        let mut state = self.lock();
        loop {
            if state.users == 0 {
                return;
            }
            let now = Instant::now();
            let mut due = Vec::new();
            while let Some(&Reverse((deadline, key))) = state.heap.peek() {
                if deadline > now {
                    break;
                }
                state.heap.pop();
                // The slot goes with the deadline: a later poll finds none and
                // is ready.
                if let Some(Some(waker)) = state.wakers.remove(&key) {
                    due.push(waker);
                }
            }
            if !due.is_empty() {
                drop(state);
                // Woken without the lock held, since a waker may poll.
                due.into_iter().for_each(Waker::wake);
                state = self.lock();
                continue;
            }
            let next = state.heap.peek().map(|&Reverse((deadline, _))| deadline);
            state = match next {
                Some(deadline) => {
                    self.changed
                        .wait_timeout(state, deadline.saturating_duration_since(now))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    /// How many deadlines and wakers the timer holds; for the tests.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, usize) {
        let state = self.lock();
        let wakers = state.wakers.values().filter(|w| w.is_some()).count();
        (state.heap.len(), wakers)
    }
}
