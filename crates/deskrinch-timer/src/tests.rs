use std::future::{pending, poll_fn};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Wake, Waker};
use std::thread;

use super::*;

/// Runs `future` on this thread, parking between polls.
fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Arc::new(Unpark(thread::current())).into();
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

/// A waker that counts its wakes.
#[derive(Default)]
struct Count(AtomicUsize);

impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_sleep_lasts_at_least_its_duration() {
    let timer = Timer::start().unwrap();
    let started = Instant::now();
    block_on(timer.sleep(ms(30)));
    assert!(started.elapsed() >= ms(30));
}

/// A waker that records its name when woken.
struct Record(u64, Arc<Mutex<Vec<u64>>>);

impl Wake for Record {
    fn wake(self: Arc<Self>) {
        self.1.lock().unwrap().push(self.0);
    }
}

#[test]
fn deadlines_fire_in_order() {
    let timer = Timer::start().unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    // All four deadlines are set before any is reached (a second out, so a
    // stall while setting up cannot overrun them), and the thread wakes
    // them in deadline order.
    let base = Instant::now() + Duration::from_secs(1);
    let mut sleeps: Vec<Sleep> = [30, 10, 20, 0]
        .into_iter()
        .map(|offset| {
            let mut sleep = timer.sleep_until(base + ms(offset));
            let waker = Waker::from(Arc::new(Record(offset, Arc::clone(&order))));
            assert!(
                Pin::new(&mut sleep)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            sleep
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    while order.lock().unwrap().len() < 4 {
        assert!(Instant::now() < deadline, "not every sleep was woken");
        thread::sleep(ms(10));
    }
    assert_eq!(*order.lock().unwrap(), [0, 10, 20, 30]);
    sleeps.clear();
}

#[test]
fn a_dropped_future_is_never_woken_and_leaves_nothing_behind() {
    let timer = Timer::start().unwrap();
    let count = Arc::new(Count::default());
    let waker = Waker::from(Arc::clone(&count));
    // A second out, so a stall cannot make the first poll ready.
    let mut sleep = timer.sleep(Duration::from_secs(1));
    assert_eq!(
        Pin::new(&mut sleep).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    );
    assert_eq!(timer.shared.held(), (1, 1));
    drop(sleep);
    assert_eq!(timer.shared.held(), (0, 0));
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
}

/// Waits until `done` holds, failing after a generous limit.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(ms(5));
    }
}

#[test]
fn a_polled_sleep_is_woken_once_when_its_deadline_passes() {
    let timer = Timer::start().unwrap();
    let count = Arc::new(Count::default());
    let waker = Waker::from(Arc::clone(&count));
    let mut sleep = timer.sleep(Duration::from_secs(1));
    let mut cx = Context::from_waker(&waker);
    assert_eq!(Pin::new(&mut sleep).poll(&mut cx), Poll::Pending);
    eventually("the sleep was never woken", || {
        count.0.load(Ordering::SeqCst) >= 1
    });
    // And only once.
    thread::sleep(ms(100));
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(Pin::new(&mut sleep).poll(&mut cx), Poll::Ready(()));
}

#[test]
fn a_deadline_taken_before_the_first_poll_is_reported_passed_and_keeps_no_waker() {
    // The race of a poll with the thread, made deterministic: the thread
    // takes the deadline while nobody has polled, then the poll comes.
    let timer = Timer::start().unwrap();
    let key = timer.shared.insert(Instant::now());
    eventually("the thread never took the deadline", || {
        timer.shared.held() == (0, 0)
    });
    assert!(!timer.shared.set_waker(key, Waker::noop()));
    assert_eq!(timer.shared.held(), (0, 0));
    // A deadline still to come keeps its waker.
    let later = timer
        .shared
        .insert(Instant::now() + Duration::from_secs(60));
    assert!(timer.shared.set_waker(later, Waker::noop()));
    assert_eq!(timer.shared.held(), (1, 1));
    timer.shared.remove(key);
    timer.shared.remove(later);
    assert_eq!(timer.shared.held(), (0, 0));
}

#[test]
fn a_timeout_returns_the_future_s_value_when_it_wins() {
    let timer = Timer::start().unwrap();
    let quick = timer.sleep(ms(10));
    assert_eq!(
        block_on(timer.timeout(ms(10_000), async move {
            quick.await;
            7
        })),
        Ok(7)
    );
    // Ready at once wins even with no time at all.
    assert_eq!(block_on(timer.timeout(Duration::ZERO, async { 8 })), Ok(8));
}

#[test]
fn a_timeout_returns_elapsed_when_it_loses() {
    let timer = Timer::start().unwrap();
    let started = Instant::now();
    assert_eq!(
        block_on(timer.timeout(ms(30), pending::<()>())),
        Err(Elapsed)
    );
    assert!(started.elapsed() >= ms(30));
}

#[test]
fn many_sleeps_share_one_thread() {
    let timer = Timer::start().unwrap();
    let mut sleeps: Vec<Option<Sleep>> = (0..1000).map(|i| Some(timer.sleep(ms(i % 50)))).collect();
    let started = Instant::now();
    block_on(poll_fn(|cx| {
        for slot in &mut sleeps {
            if let Some(sleep) = slot
                && Pin::new(sleep).poll(cx).is_ready()
            {
                *slot = None;
            }
        }
        if sleeps.iter().all(Option::is_none) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(timer.shared.held(), (0, 0));
}

#[test]
fn the_thread_ends_with_the_last_handle_and_future() {
    let timer = Timer::start().unwrap();
    let clone = timer.clone();
    let sleep = clone.sleep(ms(10_000));
    let weak: Weak<Shared> = Arc::downgrade(&timer.shared);
    drop(timer);
    drop(clone);
    // The sleep keeps the thread.
    thread::sleep(ms(30));
    assert!(weak.upgrade().is_some());
    drop(sleep);
    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() {
        assert!(Instant::now() < deadline, "the timer's thread did not end");
        thread::sleep(ms(5));
    }
}
