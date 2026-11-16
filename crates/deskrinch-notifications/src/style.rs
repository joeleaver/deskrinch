//! The popups' look: rinch's theme variables only (`deskrinch_theme::look`),
//! dark or light as the theme follows the desktop's colour scheme, and the
//! layout constants of [`policy`](crate::policy). No colour or metric of
//! the daemon's own.
//!
//! Nothing paints but a popup: the page, the column and the gaps between
//! popups are transparent, so that a surface that takes only its painted
//! pixels takes exactly the popups.

use crate::policy::{BODY_LINES, ICON_SIZE, WIDTH};

/// The window's stylesheet.
pub(crate) fn sheet() -> String {
    format!(
        "body {{ background: transparent; margin: 0; }}
.column {{
    box-sizing: border-box; width: {WIDTH}px;
    display: flex; flex-direction: column;
    gap: var(--rinch-spacing-sm);
    padding: var(--rinch-spacing-sm);
}}
.popup {{
    box-sizing: border-box;
    display: flex; flex-direction: column;
    gap: var(--rinch-spacing-xs);
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-md);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-default);
    box-shadow: var(--rinch-shadow-sm);
    font-size: var(--rinch-font-size-sm);
}}
.popup.critical {{ border-color: var(--rinch-color-red-6); }}
.header {{
    display: flex; flex-direction: row; align-items: center;
    gap: var(--rinch-spacing-xs);
    color: var(--rinch-color-dimmed);
    font-size: var(--rinch-font-size-xs);
}}
.app {{ flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.icon {{ width: {ICON_SIZE}px; height: {ICON_SIZE}px; flex: none; }}
.close {{
    flex: none; border: none; background: transparent;
    color: var(--rinch-color-dimmed); font-size: var(--rinch-font-size-md);
    padding: 0 var(--rinch-spacing-xs);
}}
.content {{ display: flex; flex-direction: row; gap: var(--rinch-spacing-sm); }}
.content.default {{ cursor: pointer; }}
.text {{ flex: 1; min-width: 0; display: flex; flex-direction: column; }}
.summary {{
    font-weight: bold; font-size: var(--rinch-font-size-md);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}}
.body {{
    white-space: pre-wrap; overflow-wrap: anywhere;
    display: -webkit-box; -webkit-box-orient: vertical;
    -webkit-line-clamp: {BODY_LINES}; overflow: hidden;
}}
.actions {{ display: flex; flex-direction: row; flex-wrap: wrap; gap: var(--rinch-spacing-xs); }}
.action {{
    padding: var(--rinch-spacing-xs) var(--rinch-spacing-sm);
    color: var(--rinch-color-text);
    background: transparent;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-default);
}}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_no_colour_of_its_own() {
        let sheet = sheet();
        assert!(!sheet.contains('#'), "a colour literal in {sheet}");
        assert!(!sheet.contains("rgb"), "a colour literal in {sheet}");
    }
}
