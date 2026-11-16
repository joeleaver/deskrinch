//! What the panel's buttons share: their backgrounds, in the bar's
//! stylesheet, for the taskbar's windows and the switcher's workspaces
//! alike.

/// The class every panel button has.
const BUTTON: &str = "panel-button";

/// A button's classes: [`BUTTON`], and `marked` when it is the one marked
/// (the active window, the current workspace).
pub(crate) fn classes(marked: bool) -> &'static str {
    if marked {
        "panel-button marked"
    } else {
        BUTTON
    }
}

/// The buttons' backgrounds, in the bar's stylesheet rather than in each
/// button's `style`, which would win over any rule: none; the marked one's,
/// the theme's selected option; and the one under the pointer, the theme's
/// hovered option, unless it is the marked one, which keeps its mark.
///
/// What the hover relies on is CSS's `:hover` as rinch keeps it: it holds
/// while the pointer is over the button or anything in it (an icon, a
/// title), so moving within a button changes nothing; it moves from one
/// button to the next in one restyle; and it clears when the pointer leaves
/// the panel's window altogether (rinch #2672). A change restyles only the
/// button whose `:hover` changed, and paints it: no layout, and nothing of
/// the widgets' own runs. No enter or leave handler is involved.
pub(crate) const STYLESHEET: &str = "\
.panel-button { background: transparent; }
.panel-button:hover { background: var(--rinch-color-option-hover); }
.panel-button.marked, .panel-button.marked:hover { \
background: var(--rinch-color-option-selected); }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classes_name_the_rules() {
        assert_eq!(classes(false), "panel-button");
        assert_eq!(classes(true), "panel-button marked");
        assert!(STYLESHEET.contains(".panel-button.marked"));
        assert!(STYLESHEET.contains(".panel-button:hover"));
    }
}
