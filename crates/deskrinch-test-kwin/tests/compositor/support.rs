//! KWin's support information, its effects and its shortcut service, as
//! the handle reads them. What KWin composites with depends on the machine
//! (OpenGL on a GPU, `QPainter` on CI's runners), so the test prints it,
//! with KWin's version, for the record; it asserts what holds of both.

use deskrinch_test_client::Compositor;
use deskrinch_test_kwin::Kwin;
use deskrinch_test_wait::wait_for;

/// An effect no KWin has.
const NO_SUCH_EFFECT: &str = "deskrinch-no-such-effect";

/// A global shortcut of KWin's own, not an effect's, which it registers
/// whatever it composites with (listed by KWin 6.6.7 in `cargo xtask
/// session kwin`, 2026-11-23).
const KWIN_SHORTCUT: &str = "Window Raise";

#[test]
#[ignore = "needs a nested KWin: cargo xtask compositor-tests kwin"]
fn kwin_says_what_it_composites_with_and_which_effects_it_runs() {
    assert_eq!(Compositor::current(), Compositor::Kwin);
    let kwin = Kwin::here().unwrap();
    let version = kwin.version().unwrap();
    let compositing = kwin.compositing_type().unwrap();
    println!("KWin {version} composites with {compositing:?}");
    let Some(compositing) = compositing else {
        panic!("KWin {version} names no compositing type");
    };
    assert!(
        ["OpenGL", "QPainter"].contains(&compositing.as_str()),
        "KWin {version}: {compositing}"
    );
    assert!(
        !kwin.effect_loaded(NO_SUCH_EFFECT).unwrap(),
        "KWin {version}"
    );
    for active in kwin.active_effects().unwrap() {
        assert!(
            kwin.effect_loaded(&active).unwrap(),
            "KWin {version}: {active} active but not loaded"
        );
    }
    wait_for(
        &format!("KWin {version} to register {KWIN_SHORTCUT}"),
        || {
            let names = kwin
                .kwin_shortcut_names()
                .map_err(|error| error.to_string())?;
            if names.iter().any(|name| name == KWIN_SHORTCUT) {
                Ok(())
            } else {
                Err(format!("{} names", names.len()))
            }
        },
    );
}
