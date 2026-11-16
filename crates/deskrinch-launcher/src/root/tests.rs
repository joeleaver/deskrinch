//! The prepare requests the root makes ([`prepare_while_hidden`]), with the
//! wiring's show and hide requests, mounted in rinch's test harness
//! (`rinch::testing::TestApp`) as [`launcher`] mounts them, in the
//! launcher's window ([`crate::surface::window_props`]): which requests the
//! launcher makes of its window, and in what order. The rest of the root
//! (the installed applications, the icon threads, the record of starts) is
//! not mounted; these tests give the model and the index themselves.
//!
//! That a real runtime honours a prepare on a real layer surface (the
//! prepared frame, nothing presented while hidden) is the compositor
//! tests' (`tests/compositor/prepared.rs`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_launcher::{Model, Row, Visibility};
use deskrinch_theme::Scheme;
use rinch::prelude::*;
use rinch::testing::{TestApp, WindowRequest};

use super::prepare_while_hidden;
use crate::surface;
use crate::wiring::{self, Wiring};

/// The wiring and the preparation mounted, shown or hidden as `shown`
/// says.
struct Mounted {
    app: TestApp,
    model: Signal<Model>,
    visibility: Arc<Visibility>,
}

fn mounted(shown: bool) -> Mounted {
    let visibility = Arc::new(Visibility::new(shown));
    let model_slot: Rc<RefCell<Option<Signal<Model>>>> = Rc::default();
    let (v, slot) = (Arc::clone(&visibility), Rc::clone(&model_slot));
    let app = TestApp::builder(move |__scope: &mut RenderScope| {
        let model = Signal::new(Model::new());
        *slot.borrow_mut() = Some(model);
        let index = Signal::new(None);
        let shown = v.signal();
        let content = wiring::wire(
            __scope,
            Wiring {
                model,
                entries: Memo::new(|| Arc::new(Vec::new())),
                index,
                visibility: v,
                shown,
                made_shown: shown.get(),
                act: Rc::new(|_| {}),
            },
        );
        prepare_while_hidden(model, index, Memo::new(|| Scheme::Dark), shown);
        content
    })
    .window_props(surface::window_props(shown))
    .mount();
    let model = model_slot.borrow().expect("made at the mount");
    Mounted {
        app,
        model,
        visibility,
    }
}

impl Mounted {
    /// Runs the loop until it is idle; answers the window requests made
    /// since the last call.
    fn requests(&mut self) -> Vec<WindowRequest> {
        self.app.settle();
        self.app.take_window_requests()
    }
}

/// A frequent application, as the record of starts makes one.
fn row(name: &str) -> Row {
    Row {
        id: DesktopId::new(format!("org.example.{name}.desktop")).unwrap(),
        name: name.to_owned(),
        icon: None,
    }
}

#[test]
fn started_hidden_it_prepares_then_a_show_shows_and_a_hide_prepares_after_it() {
    let mut mounted = mounted(false);
    assert_eq!(mounted.requests(), [WindowRequest::Prepare(None)]);
    mounted.visibility.show();
    assert_eq!(mounted.requests(), [WindowRequest::Show]);
    mounted.visibility.hide();
    assert_eq!(
        mounted.requests(),
        [WindowRequest::Hide, WindowRequest::Prepare(None)]
    );
}

#[test]
fn made_shown_it_prepares_nothing_until_its_first_hide() {
    let mut mounted = mounted(true);
    assert_eq!(mounted.requests(), []);
    mounted.visibility.hide();
    assert_eq!(
        mounted.requests(),
        [WindowRequest::Hide, WindowRequest::Prepare(None)]
    );
}

#[test]
fn what_the_next_show_draws_changing_prepares_while_hidden_only() {
    let mut mounted = mounted(false);
    assert_eq!(mounted.requests(), [WindowRequest::Prepare(None)]);
    mounted.model.update(|m| m.set_frequent(vec![row("Plain")]));
    assert_eq!(mounted.requests(), [WindowRequest::Prepare(None)]);

    mounted.visibility.show();
    assert_eq!(mounted.requests(), [WindowRequest::Show]);
    mounted
        .model
        .update(|m| m.set_frequent(vec![row("Plain"), row("Other")]));
    assert_eq!(mounted.requests(), [], "a change while shown prepares");
    mounted.visibility.hide();
    assert_eq!(
        mounted.requests(),
        [WindowRequest::Hide, WindowRequest::Prepare(None)]
    );
}
