//! Which installed application a window belongs to, by the application id
//! the window carries (on Wayland its `xdg_toplevel` app id; through
//! Xwayland its X11 class).

use crate::entry::{DesktopEntry, EntryKind};

/// The application entry the windows with `app_id` belong to, from
/// `entries`. The first rule that finds one wins:
///
/// 1. the entry whose desktop-file ID is `<app_id>.desktop`, the
///    specification's own rule for applications that follow it;
/// 2. the entry whose `StartupWMClass` is `app_id`, for applications whose
///    windows carry another name;
/// 3. the same two, ignoring case (`firefox` and `Firefox.desktop`, X11
///    classes such as `Gimp-2.10`).
///
/// A `Hidden` entry (one deleted for this user) never matches. A `NoDisplay`
/// one does: its application is not offered in menus, but its windows still
/// belong to it. An empty `app_id` matches nothing.
pub fn entry_for_app_id<'a, I>(entries: I, app_id: &str) -> Option<&'a DesktopEntry>
where
    I: IntoIterator<Item = &'a DesktopEntry>,
    I::IntoIter: Clone,
{
    if app_id.is_empty() {
        return None;
    }
    let candidates = entries
        .into_iter()
        .filter(|entry| !entry.hidden && matches!(entry.kind, EntryKind::Application(_)));
    let by_id = |same: &dyn Fn(&str, &str) -> bool| {
        candidates
            .clone()
            .find(|entry| same(entry.id.app_id(), app_id))
    };
    let by_class = |same: &dyn Fn(&str, &str) -> bool| {
        candidates.clone().find(|entry| match &entry.kind {
            EntryKind::Application(app) => app
                .startup_wm_class
                .as_deref()
                .is_some_and(|class| same(class, app_id)),
            _ => false,
        })
    };
    let exactly = |a: &str, b: &str| a == b;
    let ignoring_case = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    by_id(&exactly)
        .or_else(|| by_class(&exactly))
        .or_else(|| by_id(&ignoring_case))
        .or_else(|| by_class(&ignoring_case))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DesktopId;

    fn entry(id: &str, extra: &str) -> DesktopEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName={id}\nExec=true\n{extra}");
        let id = DesktopId::new(format!("{id}.desktop")).unwrap();
        DesktopEntry::parse(id, "/x".into(), &text).unwrap()
    }

    fn found<'a>(entries: &'a [DesktopEntry], app_id: &str) -> Option<&'a str> {
        entry_for_app_id(entries, app_id).map(|entry| entry.id.app_id())
    }

    #[test]
    fn the_desktop_file_name_comes_first() {
        let entries = [
            entry("other", "StartupWMClass=org.example.Editor\n"),
            entry("org.example.Editor", ""),
        ];
        assert_eq!(
            found(&entries, "org.example.Editor"),
            Some("org.example.Editor")
        );
    }

    #[test]
    fn then_the_startup_window_class() {
        let entries = [entry("org.example.Editor", "StartupWMClass=editor\n")];
        assert_eq!(found(&entries, "editor"), Some("org.example.Editor"));
    }

    #[test]
    fn then_either_ignoring_case() {
        let entries = [
            entry("Firefox", ""),
            entry("org.gimp.GIMP", "StartupWMClass=Gimp-2.10\n"),
        ];
        assert_eq!(found(&entries, "firefox"), Some("Firefox"));
        assert_eq!(found(&entries, "gimp-2.10"), Some("org.gimp.GIMP"));
    }

    #[test]
    fn an_exact_match_beats_one_ignoring_case() {
        let entries = [
            entry("Editor", ""),
            entry("other", "StartupWMClass=editor\n"),
        ];
        assert_eq!(found(&entries, "editor"), Some("other"));
    }

    #[test]
    fn a_hidden_entry_never_matches_and_a_no_display_one_does() {
        let entries = [
            entry("gone", "Hidden=true\n"),
            entry("helper", "NoDisplay=true\n"),
        ];
        assert_eq!(found(&entries, "gone"), None);
        assert_eq!(found(&entries, "helper"), Some("helper"));
    }

    #[test]
    fn an_empty_or_unknown_app_id_matches_nothing() {
        let entries = [entry("a", "StartupWMClass=\n")];
        assert_eq!(found(&entries, ""), None);
        assert_eq!(found(&entries, "b"), None);
    }
}
