//! A window closed and the next opened at once, over and over: each is
//! mapped, and the client keeps to the protocol whatever events of the
//! closed one are still on their way (the configure every compositor sends
//! again after the map, still being handled while the test drops the
//! window). Layer surfaces have no such window (`layer.rs`'s doc says why),
//! so they are not run here.

use std::time::{Duration, Instant};

use deskrinch_test_client::{Compositor, Paint, TestClient};
use deskrinch_test_wayland::Rgba;

/// How many windows the test opens and closes. The race it guards is
/// met about once in seven hundred windows under labwc: without the fix,
/// 400 rounds failed 2 runs in 5, and 4000 failed 5 in 5.
const ROUNDS: u32 = 4000;
/// The most all of a test's rounds may take: about ten milliseconds a
/// window here, so a compositor that stops answering is told from a slow
/// one with room to spare.
const WITHIN: Duration = Duration::from_secs(300);

/// Runs `round` `ROUNDS` times, failing with a sentence if the rounds take
/// longer than `WITHIN` or the connection fails; `what` names the rounds.
fn rounds(what: &str, client: &TestClient, mut round: impl FnMut()) {
    let start = Instant::now();
    for done in 0..ROUNDS {
        assert!(
            start.elapsed() < WITHIN,
            "the compositor stopped answering: {done} of {ROUNDS} {what} in {} s",
            WITHIN.as_secs()
        );
        round();
        assert!(
            client.is_connected(),
            "the connection failed after {} {what}",
            done + 1
        );
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_closed_and_the_next_opened_at_once_keeps_to_the_protocol() {
    Compositor::current();
    let client = TestClient::connect().unwrap();
    rounds("windows", &client, || {
        let window = client
            .open_painted(
                "close then open",
                "org.deskrinch.CloseThenOpen",
                Paint::Solid(Rgba::opaque(40, 40, 40)),
                None,
            )
            .unwrap();
        drop(window);
    });
}
