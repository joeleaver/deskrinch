//! The window list, and how protocol events fold into it.
//!
//! Pure: no Wayland here. A backend translates its protocol's events into
//! [`Change`]s on a window and tells the list when the compositor has sent a
//! consistent state ([`WindowList::commit`]); the list decides what the
//! subscriber hears. Protocols stage changes differently (wlroots batches them
//! until `done`, KDE applies each at once after `initial_state`), and both map
//! onto the same two steps.

use std::collections::BTreeMap;

use crate::window::{Event, Window, WindowId, WindowState};

/// One reported fact about a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    /// Its title.
    Title(String),
    /// Its application id.
    AppId(String),
    /// Its whole state.
    State(WindowState),
    /// It entered the named output.
    OutputEnter(String),
    /// It left the named output.
    OutputLeave(String),
}

/// A window the protocol knows about.
#[derive(Debug)]
struct Entry {
    /// What the compositor has reported, committed or not.
    pending: Window,
    /// What the subscriber was last told, once it has been told.
    announced: Option<Window>,
}

/// The windows of one connection.
#[derive(Debug, Default)]
pub(crate) struct WindowList {
    entries: BTreeMap<WindowId, Entry>,
}

impl WindowList {
    /// Starts tracking a window the protocol has just created. It is not
    /// announced until its first [`commit`](Self::commit).
    pub(crate) fn insert(&mut self, id: WindowId) {
        self.entries.insert(
            id,
            Entry {
                pending: Window::new(id),
                announced: None,
            },
        );
    }

    /// Records a change without announcing it. A change to an unknown window
    /// is ignored.
    pub(crate) fn stage(&mut self, id: WindowId, change: Change) {
        if let Some(entry) = self.entries.get_mut(&id) {
            apply(&mut entry.pending, change);
        }
    }

    /// Announces what has been staged: [`Event::Added`] the first time,
    /// [`Event::Changed`] when it differs from what was last announced, and
    /// nothing otherwise.
    pub(crate) fn commit(&mut self, id: WindowId) -> Option<Event> {
        let entry = self.entries.get_mut(&id)?;
        let event = match &entry.announced {
            None => Event::Added(entry.pending.clone()),
            Some(announced) if *announced == entry.pending => return None,
            Some(_) => Event::Changed(entry.pending.clone()),
        };
        entry.announced = Some(entry.pending.clone());
        Some(event)
    }

    /// Stages a change and, if the window has already been announced,
    /// commits it: for protocols whose changes are atomic once the initial
    /// state is in.
    pub(crate) fn apply_live(&mut self, id: WindowId, change: Change) -> Option<Event> {
        self.stage(id, change);
        let announced = self.entries.get(&id)?.announced.is_some();
        if announced { self.commit(id) } else { None }
    }

    /// Stops tracking a window. [`Event::Removed`] if it had been announced.
    pub(crate) fn remove(&mut self, id: WindowId) -> Option<Event> {
        let entry = self.entries.remove(&id)?;
        entry.announced.map(|_| Event::Removed(id))
    }

    /// An output went away: every window leaves it, and those already
    /// announced are re-announced.
    pub(crate) fn forget_output(&mut self, name: &str) -> Vec<Event> {
        let affected: Vec<_> = self
            .entries
            .iter_mut()
            .filter_map(|(&id, entry)| {
                let left = entry.pending.outputs.remove(name);
                (left && entry.announced.is_some()).then_some(id)
            })
            .collect();
        affected
            .into_iter()
            .filter_map(|id| self.commit(id))
            .collect()
    }

    /// The announced windows, in the order they appeared.
    pub(crate) fn snapshot(&self) -> Vec<Window> {
        self.entries
            .values()
            .filter_map(|entry| entry.announced.clone())
            .collect()
    }

    /// Forgets every window, without events: the connection ended.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

fn apply(window: &mut Window, change: Change) {
    match change {
        Change::Title(title) => window.title = title,
        Change::AppId(app_id) => window.app_id = app_id,
        Change::State(state) => window.state = state,
        Change::OutputEnter(output) => {
            window.outputs.insert(output);
        }
        Change::OutputLeave(output) => {
            window.outputs.remove(&output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added(event: Option<Event>) -> Window {
        match event {
            Some(Event::Added(window)) => window,
            other => panic!("expected Added, got {other:?}"),
        }
    }

    fn changed(event: Option<Event>) -> Window {
        match event {
            Some(Event::Changed(window)) => window,
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    fn tracked() -> (WindowList, WindowId) {
        let mut list = WindowList::default();
        let id = WindowId::next();
        list.insert(id);
        (list, id)
    }

    #[test]
    fn a_window_is_announced_on_its_first_commit_with_everything_staged() {
        let (mut list, id) = tracked();
        list.stage(id, Change::Title("Editor".into()));
        list.stage(id, Change::AppId("org.example.Editor".into()));
        list.stage(id, Change::State(WindowState::ACTIVE));
        list.stage(id, Change::OutputEnter("DP-1".into()));
        assert_eq!(list.snapshot(), [], "nothing announced before commit");

        let window = added(list.commit(id));
        assert_eq!(window.id, id);
        assert_eq!(window.title, "Editor");
        assert_eq!(window.app_id, "org.example.Editor");
        assert_eq!(window.state, WindowState::ACTIVE);
        assert_eq!(window.outputs.iter().collect::<Vec<_>>(), ["DP-1"]);
        assert_eq!(list.snapshot(), [window]);
    }

    #[test]
    fn a_later_commit_reports_the_new_state_in_full() {
        let (mut list, id) = tracked();
        list.stage(id, Change::Title("one".into()));
        added(list.commit(id));
        list.stage(id, Change::Title("two".into()));
        list.stage(id, Change::State(WindowState::MINIMIZED));
        let window = changed(list.commit(id));
        assert_eq!(window.title, "two");
        assert_eq!(window.state, WindowState::MINIMIZED);
    }

    #[test]
    fn a_commit_that_changes_nothing_is_silent() {
        let (mut list, id) = tracked();
        list.stage(id, Change::Title("same".into()));
        added(list.commit(id));
        list.stage(id, Change::Title("same".into()));
        assert!(list.commit(id).is_none());
    }

    #[test]
    fn live_changes_wait_for_the_first_commit() {
        let (mut list, id) = tracked();
        assert!(list.apply_live(id, Change::Title("early".into())).is_none());
        assert_eq!(added(list.commit(id)).title, "early");
        let window = changed(list.apply_live(id, Change::Title("late".into())));
        assert_eq!(window.title, "late");
    }

    #[test]
    fn removing_an_announced_window_reports_it() {
        let (mut list, id) = tracked();
        added(list.commit(id));
        assert!(matches!(list.remove(id), Some(Event::Removed(gone)) if gone == id));
        assert!(list.remove(id).is_none(), "it is gone");
        assert_eq!(list.snapshot(), []);
    }

    #[test]
    fn a_window_removed_before_it_was_announced_is_never_heard_of() {
        let (mut list, id) = tracked();
        list.stage(id, Change::Title("brief".into()));
        assert!(list.remove(id).is_none());
        assert!(
            list.commit(id).is_none(),
            "a removed window cannot come back"
        );
    }

    #[test]
    fn changes_to_unknown_windows_are_ignored() {
        let mut list = WindowList::default();
        let stranger = WindowId::next();
        list.stage(stranger, Change::Title("who".into()));
        assert!(
            list.apply_live(stranger, Change::Title("who".into()))
                .is_none()
        );
        assert!(list.commit(stranger).is_none());
        assert!(list.remove(stranger).is_none());
    }

    #[test]
    fn leaving_an_output_removes_it() {
        let (mut list, id) = tracked();
        list.stage(id, Change::OutputEnter("DP-1".into()));
        list.stage(id, Change::OutputEnter("HDMI-A-1".into()));
        list.stage(id, Change::OutputLeave("DP-1".into()));
        let window = added(list.commit(id));
        assert_eq!(window.outputs.iter().collect::<Vec<_>>(), ["HDMI-A-1"]);
    }

    #[test]
    fn a_vanished_output_is_left_by_announced_windows_with_an_event() {
        let mut list = WindowList::default();
        let [on, off, unannounced] = [WindowId::next(), WindowId::next(), WindowId::next()];
        for id in [on, off, unannounced] {
            list.insert(id);
        }
        list.stage(on, Change::OutputEnter("DP-1".into()));
        list.stage(unannounced, Change::OutputEnter("DP-1".into()));
        added(list.commit(on));
        added(list.commit(off));

        let events = list.forget_output("DP-1");
        assert_eq!(events.len(), 1, "{events:?}");
        let window = changed(events.into_iter().next());
        assert_eq!(window.id, on);
        assert!(window.outputs.is_empty());
        assert!(
            added(list.commit(unannounced)).outputs.is_empty(),
            "an unannounced window forgets the output too"
        );
    }

    #[test]
    fn the_snapshot_lists_windows_in_the_order_they_appeared() {
        let mut list = WindowList::default();
        let ids = [WindowId::next(), WindowId::next(), WindowId::next()];
        for id in ids {
            list.insert(id);
        }
        for id in ids.iter().rev() {
            added(list.commit(*id));
        }
        let order: Vec<_> = list.snapshot().iter().map(|window| window.id).collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn clear_forgets_everything_silently() {
        let (mut list, id) = tracked();
        added(list.commit(id));
        list.clear();
        assert_eq!(list.snapshot(), []);
        assert!(list.remove(id).is_none());
    }
}
