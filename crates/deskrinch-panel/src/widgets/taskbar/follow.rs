//! The window list followed: the taskbar's model, kept where the
//! window-management crate's events arrive, and its buttons handed to the
//! UI.
//!
//! Each event is applied to the model on the window-management thread, in
//! the order the subscriber hears them, under the model's lock: nothing
//! queues between the two, so nothing is dropped or reordered. The UI is
//! handed the buttons that result, as a whole, through a [`Handover`],
//! which keeps the newest: an older list of buttons is of no use once a
//! newer one exists. The UI's presses and releases take the same lock.
//!
//! The window list is followed on the panel's shared connection, made off
//! the first frame's path ([`crate::compositor`]): the taskbar is empty
//! until the compositor has answered with its windows, and stays empty,
//! with why reported once, when it offers none.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_component::{Handover, Inbox, handover, report};
use deskrinch_taskbar::{Answer, Button, Change, Input, TaskWindow, Taskbar};
use deskrinch_window_management::{
    Client, Event, Window, WindowId, WindowManagement, WindowManager, connect_with,
};

use crate::compositor::Follow;

/// The buttons the UI draws.
pub(crate) type Buttons = Arc<Vec<Button<WindowId>>>;

/// The window list followed for the taskbar.
pub(crate) struct Following {
    model: Arc<Mutex<Taskbar<WindowId>>>,
    /// `None` until connected, and when the compositor offers no window
    /// list (reported).
    manager: Arc<Mutex<Option<WindowManager>>>,
}

impl Following {
    /// The window list to follow, empty until the panel's shared connection
    /// is handed to the [`Follow`] returned beside it
    /// ([`crate::compositor`]). When the taskbar cannot follow the windows,
    /// it shows nothing, and why is reported once. Beside it, the UI's half
    /// of the buttons' hand-over, which the panel's root turns into the
    /// buttons' signal, once (none until the compositor has answered), for
    /// every taskbar to read.
    pub(crate) fn new() -> (Self, Inbox<Buttons>, Follow) {
        let model = Arc::new(Mutex::new(Taskbar::new()));
        let (buttons, inbox) = handover();
        let manager = Arc::new(Mutex::new(None));
        let (follow_model, follow_manager) = (Arc::clone(&model), Arc::clone(&manager));
        let connect: Follow = Box::new(move |client| {
            follow(client, &follow_model, &buttons, &follow_manager);
        });
        (Self { model, manager }, inbox, connect)
    }

    /// A press or a release from the UI; what it adds up to is requested of
    /// the compositor, which then reports the outcome as events.
    pub(crate) fn input(&self, input: Input<WindowId>) {
        // The lock is let go before the requests go out.
        let answer = lock(&self.model).input(input);
        let Answer::Perform(id, actions) = answer else {
            return;
        };
        let manager = lock(&self.manager);
        let Some(manager) = manager.as_ref() else {
            return;
        };
        for action in actions {
            if let Err(error) = manager.perform(id, action) {
                report::error(&format!("asking to {action} {id}"), &error);
                return;
            }
        }
    }
}

/// Follows the window list on `client`, applying each event to `model` and
/// handing `buttons` what results, and keeps the connection in `manager`;
/// with no `client` (no connection could be made, reported), the taskbar is
/// unavailable.
fn follow(
    client: Option<&Client>,
    model: &Arc<Mutex<Taskbar<WindowId>>>,
    buttons: &Handover<Buttons>,
    manager: &Mutex<Option<WindowManager>>,
) {
    let Some(client) = client else {
        *lock(model) = Taskbar::unavailable();
        return;
    };
    let applied = Arc::clone(model);
    let deliver = buttons.clone();
    let connected = connect_with(client, move |event| {
        let Some(change) = change(event) else {
            return;
        };
        let mut model = lock(&applied);
        let before = model.buttons();
        if let Answer::Ended(reason) = model.input(Input::Change(change)) {
            report::error("following the windows (the taskbar is empty)", &reason);
        }
        let after = model.buttons();
        if after != before {
            deliver.deliver(Arc::new(after));
        }
    });
    match connected {
        Ok(connected) => {
            let capabilities = connected.capabilities();
            tracing::info!("following the windows through {:?}", capabilities.protocol);
            lock(model).connected(capabilities.actions);
            *lock(manager) = Some(connected);
        }
        Err(error) => {
            report::error("following the windows (the taskbar shows none)", &error);
            *lock(model) = Taskbar::unavailable();
        }
    }
}

/// The taskbar's change for a window-management event; `None` for an event
/// the taskbar does not know.
fn change(event: Event) -> Option<Change<WindowId>> {
    Some(match event {
        Event::Added(window) => Change::Added(task_window(window)),
        Event::Changed(window) => Change::Changed(task_window(window)),
        Event::Removed(id) => Change::Removed(id),
        Event::Ended(reason) => Change::Ended(reason),
        _ => return None,
    })
}

/// What the taskbar uses of `window`.
fn task_window(window: Window) -> TaskWindow<WindowId> {
    TaskWindow {
        id: window.id,
        title: window.title,
        app_id: window.app_id,
        state: window.state,
        parent: window.parent,
        outputs: window.outputs,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
