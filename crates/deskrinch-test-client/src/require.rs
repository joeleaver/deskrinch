//! A test that needs particular outputs, checking it has them.

use crate::{Compositor, TestClient};

/// Panics unless the nested session's outputs have the modes `wanted`
/// (width and height in device pixels), in the order the session made
/// them: a test written for particular outputs calls it first, so that run
/// on others (outside its row's pass in `cargo xtask compositor-tests`) it
/// fails saying so rather than testing what it was not written for.
///
/// # Panics
///
/// When the outputs differ, saying what the test needs, what the session
/// has, and how to run it; outside a nested session, as
/// [`Compositor::current`]; and when the session cannot be reached.
pub fn require_outputs(wanted: &[(i32, i32)]) {
    let _nested = Compositor::current();
    let client = TestClient::connect()
        .unwrap_or_else(|error| panic!("the session, to read its outputs: {error}"));
    let has: Vec<Option<(i32, i32)>> = client
        .output_names()
        .iter()
        .map(|name| client.output_info(name).and_then(|info| info.mode))
        .collect();
    let matches = has.len() == wanted.len()
        && has
            .iter()
            .zip(wanted)
            .all(|(mode, wanted)| *mode == Some(*wanted));
    assert!(matches, "{}", mismatch_sentence(wanted, &has));
}

/// What a test that needs `wanted` says on outputs that `has`.
fn mismatch_sentence(wanted: &[(i32, i32)], has: &[Option<(i32, i32)>]) -> String {
    let wanted: Vec<String> = wanted.iter().map(|(w, h)| format!("{w}x{h}")).collect();
    let has: Vec<String> = has
        .iter()
        .map(|mode| mode.map_or_else(|| "no mode".to_owned(), |(w, h)| format!("{w}x{h}")))
        .collect();
    format!(
        "this test needs outputs {}; the session has {}: run it through its suite's own row \
         (`cargo xtask compositor-tests`, which gives that row's pass these outputs)",
        wanted.join(", "),
        if has.is_empty() {
            "none".to_owned()
        } else {
            has.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_it_needs_what_there_is_and_how_to_run_it() {
        let said = mismatch_sentence(&[(3840, 2160), (1920, 1080)], &[Some((1280, 720)), None]);
        assert_eq!(
            said,
            "this test needs outputs 3840x2160, 1920x1080; the session has 1280x720, no mode: run \
             it through its suite's own row (`cargo xtask compositor-tests`, which gives that \
             row's pass these outputs)"
        );
        assert!(mismatch_sentence(&[(1, 1)], &[]).contains("the session has none"));
    }
}
