//! Putting a layout back until the compositor reports it, applying it again
//! while it does not.
//!
//! One application is not always enough under labwc, by its own rule
//! (labwc 0.9.3, `output_config_apply` in `src/output.c`, on wlroots
//! 0.19): it takes a configuration's heads one at a time, in the reverse
//! of the order they were sent (wlroots inserts each at the head of the
//! configuration's list). Each head's state is committed, which lays the
//! automatically placed outputs out again (`output_layout_reconfigure`:
//! to the right of the rightmost fixed output, in the layout's order); an
//! output that is off is added automatically; and only an output whose
//! position then differs from the one stated is fixed there. So an output
//! handled early, at its stated position, stays placed automatically, and
//! a later head being fixed moves it. Seen: with both outputs placed
//! automatically, HEADLESS-2 at (0,0) and HEADLESS-1 at (1280,0), scaling
//! or turning HEADLESS-2 with both positions stated fixed HEADLESS-1 at
//! (1280,0) and left HEADLESS-2 at (2560,0). An output fixed where it is
//! stated is not moved again, so each further application fixes at least
//! one output left elsewhere, and applying the layout again ends. Under
//! sway and KWin the first application is the layout.

use std::time::{Duration, Instant};

use crate::Error;
use crate::layout::{OutputState, layout_differences};

/// How long an application has to show before the layout is applied again.
pub(crate) const SETTLE: Duration = Duration::from_secs(1);
/// How often the outputs are read while waiting.
const POLL: Duration = Duration::from_millis(50);

/// Applies `layout` with `apply`, and again while `read` does not report
/// it, each application given `settle` to show, for at most `patience`.
/// Returns how many applications it took.
///
/// # Errors
///
/// What `apply` or `read` fail with, and [`Error::LayoutNotReached`],
/// with how many applications were made and what still differs, when
/// `patience` runs out.
pub(crate) fn converge(
    layout: &[OutputState],
    mut apply: impl FnMut(&[OutputState]) -> Result<(), Error>,
    mut read: impl FnMut() -> Result<Vec<OutputState>, Error>,
    settle: Duration,
    patience: Duration,
) -> Result<u32, Error> {
    let deadline = Instant::now() + patience;
    let mut applications = 0;
    loop {
        apply(layout)?;
        applications += 1;
        let shown_by = (Instant::now() + settle).min(deadline);
        loop {
            let now = read()?;
            let Some(differences) = layout_differences(layout, &now) else {
                return Ok(applications);
            };
            if Instant::now() >= deadline {
                return Err(Error::LayoutNotReached {
                    differences,
                    patience,
                    applications,
                });
            }
            if Instant::now() >= shown_by {
                break;
            }
            std::thread::sleep(POLL.min(settle));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use deskrinch_test_wayland::Transform;

    use super::*;
    use crate::layout::Mode;

    fn output(name: &str, x: i32) -> OutputState {
        OutputState {
            name: name.to_owned(),
            enabled: true,
            position: (x, 0),
            mode: Some(Mode {
                width: 1280,
                height: 720,
                refresh_mhz: 0,
            }),
            scale: 1.0,
            transform: Transform::Normal,
        }
    }

    const SHORT: Duration = Duration::from_millis(20);

    #[test]
    fn a_layout_shown_at_once_is_applied_once() {
        let layout = vec![output("H2", 0), output("H1", 1280)];
        let applied = RefCell::new(0);
        let taken = converge(
            &layout,
            |_| {
                *applied.borrow_mut() += 1;
                Ok(())
            },
            || Ok(layout.clone()),
            SHORT,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!((taken, *applied.borrow()), (1, 1));
    }

    #[test]
    fn an_output_left_elsewhere_is_put_right_by_applying_again() {
        let layout = vec![output("H2", 0), output("H1", 1280)];
        // After the first application labwc leaves H2 to the right of the
        // fixed H1; after the second it is where it is stated.
        let displaced = vec![output("H2", 2560), output("H1", 1280)];
        let applied = RefCell::new(0_usize);
        let taken = converge(
            &layout,
            |_| {
                *applied.borrow_mut() += 1;
                Ok(())
            },
            || {
                Ok(if *applied.borrow() < 2 {
                    displaced.clone()
                } else {
                    layout.clone()
                })
            },
            SHORT,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(taken, 2);
    }

    #[test]
    fn a_layout_never_shown_fails_saying_how_often_it_was_applied() {
        let layout = vec![output("H2", 0), output("H1", 1280)];
        let elsewhere = vec![output("H2", 2560), output("H1", 1280)];
        let applied = RefCell::new(0_u32);
        let error = converge(
            &layout,
            |_| {
                *applied.borrow_mut() += 1;
                Ok(())
            },
            || Ok(elsewhere.clone()),
            SHORT,
            Duration::from_millis(200),
        )
        .unwrap_err();
        let Error::LayoutNotReached {
            differences,
            applications,
            ..
        } = &error
        else {
            panic!("{error:?}");
        };
        assert!(*applications >= 2, "{applications}");
        assert_eq!(*applications, *applied.borrow());
        assert!(differences.contains("H2 at 2560,0"), "{differences}");
        let said = error.to_string();
        assert!(
            said.contains(&format!("after {applications} applications")),
            "{said}"
        );
    }

    #[test]
    fn a_failure_to_apply_is_the_error() {
        let layout = vec![output("H1", 0)];
        let error = converge(
            &layout,
            |_| Err(Error::UnknownOutput("H1".into())),
            || Ok(layout.clone()),
            SHORT,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(error, Error::UnknownOutput(_)));
    }
}
