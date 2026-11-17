//! [`Taskbar`]: the windows followed, and what an [`Input`] does to them.

use std::collections::BTreeSet;
use std::time::Instant;

use deskrinch_gestures::Click;
use deskrinch_window_management::{Action, Actions, EndReason, WindowState};

use crate::on_bar::Placement;

/// What the taskbar uses of a window, as the compositor last reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWindow<Id> {
    /// Which window it is.
    pub id: Id,
    /// Its title.
    pub title: String,
    /// Its application id, which its icon is found by.
    pub app_id: String,
    /// Its state.
    pub state: WindowState,
    /// The window it belongs to, such as a dialog's main window.
    pub parent: Option<Id>,
    /// The outputs it is on, by name, as this report gives them; none when
    /// the compositor reports none (not yet, for an instant, or not at
    /// all). The model keeps its [`Placement`] ([`crate::on_bar`]).
    pub outputs: BTreeSet<String>,
}

/// A change to the window list.
#[derive(Debug, Clone)]
pub enum Change<Id> {
    /// A window appeared.
    Added(TaskWindow<Id>),
    /// A window changed; this is its new state in full.
    Changed(TaskWindow<Id>),
    /// A window is gone.
    Removed(Id),
    /// The compositor stopped reporting windows, or the connection was
    /// lost.
    Ended(EndReason),
}

/// A pointer button the taskbar answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// The primary button: activate, or minimize the active window.
    Primary,
    /// The middle button: close.
    Middle,
}

/// What the taskbar hears.
#[derive(Debug, Clone)]
pub enum Input<Id> {
    /// The window list changed, heard at this time: a new window's grace
    /// counts from it ([`crate::on_bar`]).
    Change(Change<Id>, Instant),
    /// A pointer button went down on the button of the window with this id,
    /// or (`None`) on the taskbar but on no button.
    Press(Option<Id>, PointerButton),
    /// A pointer button came up, on a button or (`None`) on none.
    Release(Option<Id>, PointerButton),
}

/// What the component does after an [`Input`].
#[derive(Debug, Clone)]
pub enum Answer<Id> {
    /// Nothing beyond showing the buttons as they now are.
    Stay,
    /// Request these actions of this window, in this order.
    Perform(Id, Vec<Action>),
    /// The window list ended, for this reason: log it. Answered once.
    Ended(EndReason),
}

/// One button: a window and its dialogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button<Id> {
    /// The window it stands for.
    pub id: Id,
    /// The window's title.
    pub title: String,
    /// The window's application id.
    pub app_id: String,
    /// The window or one of its dialogs is active.
    pub active: bool,
    /// The window is minimized.
    pub minimized: bool,
    /// The window or one of its dialogs asks for attention.
    pub attention: bool,
    /// Where the window is, as far as the taskbar knows: which bars show
    /// the button ([`crate::on_bar`]).
    pub placement: Placement,
}

/// Whether the taskbar is following a window list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Following,
    /// Ended, or never connected: nothing is shown, heard or done.
    Over,
}

/// A window followed, and where it is as far as the taskbar knows.
#[derive(Debug, Clone)]
struct Followed<Id> {
    window: TaskWindow<Id>,
    placement: Placement,
}

/// The taskbar: the windows in the order they were added, each with its
/// placement, the actions the connection can request, and the press held.
/// The rules are the crate's documentation.
#[derive(Debug, Clone)]
pub struct Taskbar<Id> {
    windows: Vec<Followed<Id>>,
    actions: Actions,
    status: Status,
    click: Click<Id, PointerButton>,
}

impl<Id: Copy + Eq> Taskbar<Id> {
    /// A taskbar following a window list, before the connection has said
    /// what it can do ([`connected`](Self::connected)): the windows already
    /// open arrive as [`Change::Added`] while it connects.
    #[must_use]
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            actions: Actions::empty(),
            status: Status::Following,
            click: Click::new(),
        }
    }

    /// A taskbar that could not connect: it shows nothing and does nothing.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            status: Status::Over,
            ..Self::new()
        }
    }

    /// The connection can request `actions`.
    pub fn connected(&mut self, actions: Actions) {
        self.actions = actions;
    }

    /// What `input` does.
    pub fn input(&mut self, input: Input<Id>) -> Answer<Id> {
        if self.status == Status::Over {
            return Answer::Stay;
        }
        match input {
            Input::Change(change, at) => self.change(change, at),
            Input::Press(on, button) => {
                self.click.press(on, button);
                Answer::Stay
            }
            Input::Release(on, button) => match self.click.release(on, button) {
                Some(id) if self.buttons().iter().any(|b| b.id == id) => self.clicked(id, button),
                _ => Answer::Stay,
            },
        }
    }

    fn change(&mut self, change: Change<Id>, at: Instant) -> Answer<Id> {
        match change {
            Change::Added(window) => {
                let followed = self.followed(window, at);
                self.windows
                    .retain(|known| known.window.id != followed.window.id);
                self.windows.push(followed);
            }
            Change::Changed(window) => {
                let followed = self.followed(window, at);
                if let Some(known) = self
                    .windows
                    .iter_mut()
                    .find(|known| known.window.id == followed.window.id)
                {
                    *known = followed;
                } else {
                    self.windows.push(followed);
                }
            }
            Change::Removed(id) => self.windows.retain(|known| known.window.id != id),
            Change::Ended(reason) => {
                self.windows.clear();
                self.status = Status::Over;
                return Answer::Ended(reason);
            }
        }
        Answer::Stay
    }

    /// `window` as reported at `at`, with where it is once it reports what
    /// it does ([`Placement::after`]).
    fn followed(&self, window: TaskWindow<Id>, at: Instant) -> Followed<Id> {
        let before = self
            .windows
            .iter()
            .find(|known| known.window.id == window.id)
            .map(|known| &known.placement);
        let placement = Placement::after(before, window.outputs.clone(), at);
        Followed { window, placement }
    }

    /// What a click of `button` on `id`'s button asks for.
    fn clicked(&self, id: Id, button: PointerButton) -> Answer<Id> {
        let Some(shown) = self.buttons().into_iter().find(|b| b.id == id) else {
            return Answer::Stay;
        };
        let can = |action: Action| self.actions.contains(action.into());
        let actions: Vec<Action> = match button {
            PointerButton::Middle => vec![Action::Close],
            PointerButton::Primary if shown.minimized => {
                if can(Action::Restore) {
                    vec![Action::Restore, Action::Activate]
                } else {
                    vec![Action::Activate]
                }
            }
            PointerButton::Primary if shown.active => vec![Action::Minimize],
            PointerButton::Primary => vec![Action::Activate],
        };
        let actions: Vec<Action> = actions.into_iter().filter(|&a| can(a)).collect();
        if actions.is_empty() {
            Answer::Stay
        } else {
            Answer::Perform(id, actions)
        }
    }

    /// The buttons, in the order their windows were added.
    #[must_use]
    pub fn buttons(&self) -> Vec<Button<Id>> {
        self.windows
            .iter()
            .filter(|followed| self.has_button(&followed.window))
            .map(|Followed { window, placement }| Button {
                id: window.id,
                title: window.title.clone(),
                app_id: window.app_id.clone(),
                active: self.any_of(window.id, WindowState::ACTIVE),
                minimized: window.state.contains(WindowState::MINIMIZED),
                attention: self.any_of(window.id, WindowState::DEMANDS_ATTENTION),
                placement: placement.clone(),
            })
            .collect()
    }

    /// Whether `window` has a button of its own: it does not ask to be left
    /// off, and it has no parent the taskbar knows.
    fn has_button(&self, window: &TaskWindow<Id>) -> bool {
        !window.state.contains(WindowState::SKIP_TASKBAR)
            && window
                .parent
                .is_none_or(|parent| self.window(parent).is_none())
    }

    fn window(&self, id: Id) -> Option<&TaskWindow<Id>> {
        self.windows
            .iter()
            .map(|followed| &followed.window)
            .find(|window| window.id == id)
    }

    /// Whether the window `id` or one of the windows below it (by parent,
    /// at any depth) has `flag`.
    fn any_of(&self, id: Id, flag: WindowState) -> bool {
        let mut below = vec![id];
        // At most as many rounds as windows: a cycle of parents ends.
        let mut seen: Vec<Id> = Vec::new();
        while let Some(next) = below.pop() {
            if seen.contains(&next) {
                continue;
            }
            seen.push(next);
            if self.window(next).is_some_and(|w| w.state.contains(flag)) {
                return true;
            }
            below.extend(
                self.windows
                    .iter()
                    .map(|followed| &followed.window)
                    .filter(|w| w.parent == Some(next))
                    .map(|w| w.id),
            );
        }
        false
    }
}

impl<Id: Copy + Eq> Default for Taskbar<Id> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
