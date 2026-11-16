//! The popups' look: rinch's theme variables only (`deskrinch_theme::look`),
//! dark or light as the theme follows the desktop's colour scheme, and the
//! layout constants of [`policy`](crate::policy). No colour or metric of
//! the daemon's own.
//!
//! Nothing paints but a popup and the count line's text: the page, the
//! column and the gaps between popups are transparent. A popup has no
//! shadow, which would paint into the gaps; its border sets it apart.
//!
//! The column fills the window's height, the room. The popups' container
//! is a flex column that wraps: the popups that fit whole are in its first
//! column, and the rest wrap into a second one, clipped away
//! (`overflow: hidden`). The count line sits under the container, and
//! takes its room only while it is there. A waiting popup (wrapped, or a
//! first one too tall for the room) is `visibility: hidden`: laid out,
//! never seen.
//!
//! The body is cut short with `-webkit-line-clamp` on purpose; rinch issue
//! 2748 (a removed clamped element panics the next layout) is a rinch bug
//! the daemon waits on, not a reason to clamp otherwise.

use crate::policy::{BODY_LINES, ICON_SIZE, WIDTH};

/// The window's stylesheet.
pub(crate) fn sheet() -> String {
    format!(
        "html, body {{ background: transparent; margin: 0; height: 100%; }}
.column {{
    box-sizing: border-box; width: {WIDTH}px; height: 100%;
    display: flex; flex-direction: column;
    gap: var(--rinch-spacing-sm);
    padding: var(--rinch-spacing-sm);
}}
.popups {{
    flex: 1; min-height: 0;
    display: flex; flex-direction: column; flex-wrap: wrap; align-content: flex-start;
    gap: var(--rinch-spacing-sm);
    overflow: hidden;
}}
.slot {{ width: 100%; flex: none; }}
.count {{
    flex: none;
    color: var(--rinch-color-dimmed);
    font-size: var(--rinch-font-size-sm);
}}
.popup.waiting {{ visibility: hidden; }}
.popup {{
    box-sizing: border-box;
    display: flex; flex-direction: column;
    gap: var(--rinch-spacing-xs);
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-md);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-default);
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
