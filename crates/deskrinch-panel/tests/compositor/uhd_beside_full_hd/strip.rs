//! What a bar's picture shows of a full taskbar, read against the same bar
//! before any window opened: the columns its buttons and its count of the
//! windows left out changed.
//!
//! The empty bar is its background from the switcher to the clock, so the
//! clock is what the empty bar draws right of its middle, and the taskbar
//! is what changed left of the clock. The buttons, icons alone on a full
//! bar, are alike: they repeat at one pitch from the first, each as wide as
//! the others. The count follows the last.
//!
//! The width is what tells the count from one more button. Its text is
//! centred in its own room right after the last button's gap, so where an
//! icon is centred in a button the count can start in the column a next
//! button would: with the panel's sizes (an icon 16 wide in a button 28
//! wide, a count's room 36 wide), a count 24 wide, such as `+15`, does.
//! It is then as wide as no icon is.

use std::ops::Range;

use deskrinch_test_screen::Image;

/// A full taskbar as a bar's picture shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Full {
    /// The column after the last button that repeats at the buttons'
    /// pitch; the count, and the active button when it is drawn otherwise,
    /// are right of it.
    pub(crate) buttons_end: u32,
    /// The first column of the clock, which ends the taskbar's columns.
    pub(crate) clock_left: u32,
}

/// The runs of columns in `columns` where `is` differs from `was` in any of
/// `rows`, each as its first column and the column after its last.
fn changed_runs(
    was: &Image,
    is: &Image,
    rows: &Range<u32>,
    columns: Range<u32>,
) -> Vec<Range<u32>> {
    let mut runs: Vec<Range<u32>> = Vec::new();
    for x in columns {
        if rows.clone().any(|y| was.pixel(x, y) != is.pixel(x, y)) {
            match runs.last_mut() {
                Some(run) if run.end == x => run.end = x + 1,
                _ => runs.push(x..x + 1),
            }
        }
    }
    runs
}

/// The first column of the clock in `empty`, a bar with no windows: the
/// first right of its middle, which is the empty taskbar's background, that
/// is not that background in any of `rows`.
fn clock_left(empty: &Image, rows: &Range<u32>) -> Option<u32> {
    let middle = empty.width() / 2;
    let background = empty.pixel(middle, rows.start + (rows.end - rows.start) / 2)?;
    (middle..empty.width()).find(|&x| rows.clone().any(|y| empty.pixel(x, y) != Some(background)))
}

/// Reads `full` against `empty`, the same bar before any window, in its
/// band `rows`, as a full taskbar: buttons at one pitch and of one width
/// (the most common of each), with no gap as wide as that pitch anywhere
/// up to the clock (none missing, the last included), and something drawn
/// after them that is not one more button (the count, which
/// [`count_changed`] confirms).
///
/// # Errors
///
/// What is not so, said.
pub(crate) fn full(empty: &Image, full: &Image, rows: &Range<u32>) -> Result<Full, String> {
    let clock_left = clock_left(empty, rows).ok_or("the empty bar shows no clock")?;
    let runs = changed_runs(empty, full, rows, 0..clock_left);
    let starts: Vec<u32> = runs.iter().map(|run| run.start).collect();
    let mut pitches: Vec<u32> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    if pitches.len() < 2 {
        return Err(format!("too little of a taskbar is drawn: {runs:?}"));
    }
    pitches.sort_unstable();
    let pitch = pitches[pitches.len() / 2];
    let mut widths: Vec<u32> = runs.iter().map(|run| run.end - run.start).collect();
    widths.sort_unstable();
    let width = widths[widths.len() / 2];
    let buttons = (0_u32..)
        .zip(&runs)
        .take_while(|&(k, run)| {
            run.start == runs[0].start + k * pitch && run.end - run.start == width
        })
        .count();
    if buttons < 2 || buttons == runs.len() {
        return Err(format!(
            "{buttons} buttons {width} wide at a pitch of {pitch} and nothing after them: {runs:?}"
        ));
    }
    if let Some(gap) = runs
        .windows(2)
        .map(|pair| pair[0].end..pair[1].start)
        .find(|gap| gap.end - gap.start >= pitch)
    {
        return Err(format!(
            "nothing is drawn at columns {gap:?}, a button's pitch ({pitch}) or more"
        ));
    }
    Ok(Full {
        buttons_end: runs[buttons - 1].end,
        clock_left,
    })
}

/// Whether `more`, the bar of `full` once one more window is left out,
/// differs from it left of the clock only right of the buttons: the count
/// changed, and nothing else did.
///
/// # Errors
///
/// What changed instead, said.
pub(crate) fn count_changed(
    taskbar: Full,
    full: &Image,
    more: &Image,
    rows: &Range<u32>,
) -> Result<(), String> {
    let runs = changed_runs(full, more, rows, 0..taskbar.clock_left);
    match runs.first() {
        None => Err("nothing changed for one more window left out".to_owned()),
        Some(first) if first.start < taskbar.buttons_end => Err(format!(
            "columns among the buttons (which end at {}) changed: {runs:?}",
            taskbar.buttons_end
        )),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_test_screen::Rgba;

    const BACKGROUND: Rgba = Rgba::opaque(20, 20, 20);
    const INK: Rgba = Rgba::opaque(220, 220, 220);

    /// A bar one row high, `width` wide, inked at `columns`.
    fn bar(width: u32, columns: &[Range<u32>]) -> Image {
        let pixels = (0..width)
            .map(|x| {
                if columns.iter().any(|run| run.contains(&x)) {
                    INK
                } else {
                    BACKGROUND
                }
            })
            .collect();
        Image::new(width, 1, pixels).unwrap()
    }

    /// Icons 16 wide at a pitch of 32 from column 10, `buttons` of them,
    /// the count `count` after them, and the clock at 320..330.
    fn taskbar(buttons: u32, count: &[Range<u32>]) -> Vec<Range<u32>> {
        let mut columns: Vec<Range<u32>> = (0..buttons).map(|k| 10 + 32 * k..26 + 32 * k).collect();
        columns.extend_from_slice(count);
        columns.push(320..330);
        columns
    }

    #[test]
    fn reads_a_full_taskbar_and_its_count() {
        let rows = 0..1;
        let empty = bar(340, &taskbar(0, &[]));
        let full_bar = bar(340, &taskbar(8, &[270..274, 276..280]));
        let read = full(&empty, &full_bar, &rows).unwrap();
        assert_eq!(
            read,
            Full {
                buttons_end: 250,
                clock_left: 320
            }
        );
        let more = bar(340, &taskbar(8, &[270..274, 276..281]));
        count_changed(read, &full_bar, &more, &rows).unwrap();
        // A changed button is not the count.
        let moved = bar(340, &taskbar(8, &[270..274, 276..280])[1..]);
        assert!(count_changed(read, &full_bar, &moved, &rows).is_err());
        assert!(count_changed(read, &full_bar, &full_bar, &rows).is_err());
    }

    /// The count as CI drew it after the last button (sway's `+15` read as
    /// a 57th button): starting in the column a ninth button would, but
    /// wider than an icon.
    #[test]
    fn a_count_where_a_button_would_start_is_still_the_count() {
        let rows = 0..1;
        let empty = bar(340, &taskbar(0, &[]));
        let full_bar = bar(340, &taskbar(8, std::slice::from_ref(&(266..290))));
        assert_eq!(
            full(&empty, &full_bar, &rows),
            Ok(Full {
                buttons_end: 250,
                clock_left: 320
            })
        );
    }

    #[test]
    fn a_missing_button_or_count_is_not_a_full_taskbar() {
        let rows = 0..1;
        let empty = bar(340, &taskbar(0, &[]));
        let mut gap = taskbar(8, std::slice::from_ref(&(270..274)));
        gap.remove(4);
        let error = full(&empty, &bar(340, &gap), &rows).unwrap_err();
        assert!(
            error.contains("nothing is drawn at columns 122..170"),
            "{error}"
        );
        let error = full(&empty, &bar(340, &taskbar(8, &[])), &rows).unwrap_err();
        assert!(error.contains("nothing after them"), "{error}");
        // A ninth icon after the eighth is a button, not the count.
        let error = full(&empty, &bar(340, &taskbar(9, &[])), &rows).unwrap_err();
        assert!(error.contains("9 buttons 16 wide"), "{error}");
        assert!(full(&empty, &empty, &rows).is_err());
        let no_clock = bar(340, &[]);
        assert!(full(&no_clock, &no_clock, &rows).is_err());
    }
}
