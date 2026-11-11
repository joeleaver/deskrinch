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

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use deskrinch_component::{Handover, report};
use deskrinch_taskbar::{Answer, Button, Change, Input, TaskWindow, Taskbar};
use deskrinch_window_management::{
    Event, Window, WindowId, WindowManagement, WindowManager, connect,
};
use rinch::prelude::Signal;

/// The buttons the UI draws.
pub(crate) type Buttons = Arc<Vec<Button<WindowId>>>;

/// The window list followed for the taskbar.
pub(crate) struct Following {
    model: Arc<Mutex<Taskbar<WindowId>>>,
    buttons: Arc<Handover<Buttons>>,
    initial: Buttons,
    /// `None` when the compositor offers no window list (reported).
    manager: Option<WindowManager>,
}

impl Following {
    /// Connects to the compositor's window list and follows it. When it
    /// cannot, the taskbar shows nothing, and why is reported once.
    pub(crate) fn start() -> Self {
        let model = Arc::new(Mutex::new(Taskbar::new()));
        let buttons: Arc<Handover<Buttons>> = Arc::default();
        let applied = Arc::clone(&model);
        let deliver = Arc::clone(&buttons);
        let connected = connect(move |event| {
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
        let manager = match connected {
            Ok(manager) => {
                lock(&model).connected(manager.capabilities().actions);
                Some(manager)
            }
            Err(error) => {
                report::error("following the windows (the taskbar shows none)", &error);
                *lock(&model) = Taskbar::unavailable();
                None
            }
        };
        let initial = Arc::new(lock(&model).buttons());
        Self {
            model,
            buttons,
            initial,
            manager,
        }
    }

    /// The UI's signal of the buttons, on the UI thread.
    pub(crate) fn signal(&self) -> Signal<Buttons> {
        self.buttons.signal(Arc::clone(&self.initial))
    }

    /// A press or a release from the UI; what it adds up to is requested of
    /// the compositor, which then reports the outcome as events.
    pub(crate) fn input(&self, input: Input<WindowId>) {
        // The lock is let go before the requests go out.
        let answer = lock(&self.model).input(input);
        let (Answer::Perform(id, actions), Some(manager)) = (answer, &self.manager) else {
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
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
