//! The bound on [`grow_to`]: KWin stood in for by closures, so no bus is
//! reached.

use std::cell::Cell;

use super::*;

/// A KWin that lists `start` desktops and takes creations up to `cap`;
/// `grow_to(count)` against it, and how many creations it was asked for.
fn against(start: usize, cap: usize, count: usize) -> (Result<(), Error>, usize) {
    let listed = Cell::new(start);
    let created = Cell::new(0);
    let grown = grow_to(
        count,
        || Ok(listed.get()),
        |now| {
            assert_eq!(now, listed.get(), "told how many are listed");
            created.set(created.get() + 1);
            listed.set((listed.get() + 1).min(cap));
            Ok(())
        },
    );
    (grown, created.get())
}

#[test]
fn desktops_are_created_until_kwin_lists_enough() {
    let (grown, created) = against(1, usize::MAX, 4);
    assert!(grown.is_ok(), "{grown:?}");
    assert_eq!(created, 3);
    let (grown, created) = against(6, usize::MAX, 4);
    assert!(grown.is_ok(), "{grown:?}");
    assert_eq!(created, 0, "enough already");
}

#[test]
fn a_kwin_that_caps_creations_fails_after_as_many_as_asked() {
    let (grown, created) = against(1, 2, 4);
    assert_eq!(created, 4, "at most as many creations as desktops asked");
    let error = grown.unwrap_err();
    assert!(
        matches!(
            error,
            Error::TooFewDesktops {
                asked: 4,
                listed: 2
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "asked for 4 virtual desktops; after 4 creations KWin lists 2"
    );
    let (grown, created) = against(1, 1, 3);
    assert_eq!(created, 3, "a KWin that ignores every creation");
    assert!(
        matches!(
            grown,
            Err(Error::TooFewDesktops {
                asked: 3,
                listed: 1
            })
        ),
        "{grown:?}"
    );
}
