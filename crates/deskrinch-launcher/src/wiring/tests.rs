//! The view and the wiring mounted in rinch's test harness
//! (`rinch::testing::TestApp`): input sent through rinch's own dispatch, as
//! the desktop shell sends it, and what reached the model, `act` and the
//! log read back. What needs a compositor (the window shown and hidden, the
//! activation token, the application started) is the compositor tests'. So
//! is what follows the window's keyboard focus (hiding when it is lost, the
//! has-keyboard line): `TestApp` sends the focus change but does not publish
//! it to `rinch::window::window_focused`, as the desktop shell does.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_desktop_entries::{CatalogEntry, DesktopEntry, DesktopId};
use deskrinch_launcher::{Action, Input, Model, Visibility, query_at_hide};
use deskrinch_theme::Scheme;
use rinch::prelude::*;
use rinch::testing::TestApp;

use super::{Wiring, wire};
use crate::{surface, view};

/// An application entry, shown in menus.
fn entry(id: &str, name: &str) -> Arc<CatalogEntry> {
    let text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run-{id}\n");
    let id = DesktopId::new(format!("{id}.desktop")).unwrap();
    Arc::new(CatalogEntry {
        entry: DesktopEntry::parse(id, "/x".into(), &text, &[]).unwrap(),
        visibility: deskrinch_desktop_entries::Visibility::Shown,
    })
}

/// The applications every test searches: "term" finds `term`, `terminal`
/// and `ultraterm`.
fn entries() -> Arc<Vec<Arc<CatalogEntry>>> {
    Arc::new(vec![
        entry("ultraterm", "Ultraterm"),
        entry("terminal", "Terminal"),
        entry("term", "Term"),
        entry("files", "Files"),
    ])
}

/// A slot for a signal made inside the mounted component.
type Slot<T> = Rc<RefCell<Option<T>>>;

/// What a slot was given at the mount.
fn taken<T: Copy>(slot: &Slot<T>) -> T {
    slot.borrow().expect("made at the mount")
}

/// `component` mounted as the launcher draws it: in a window of its size,
/// with the shell's look, so its padding and the field's box are the
/// launcher's own (with no look, the launcher has no padding, and its
/// corner is the field).
fn launcher_window(component: impl FnOnce(&mut RenderScope) -> NodeHandle + 'static) -> TestApp {
    TestApp::builder(component)
        .size(surface::WIDTH, surface::HEIGHT)
        .theme(deskrinch_theme::look::props(Scheme::Light))
        .mount()
}

// ── The view, its callbacks recorded ────────────────────────────────────

/// What the view's callbacks were given.
#[derive(Debug, Default)]
struct Calls {
    queries: Vec<String>,
    keys: Vec<String>,
    pointer: Vec<Input>,
}

/// The view, mounted with a model searched for "term", its callbacks
/// recording; `on_key` takes the four keys the launcher takes.
fn mounted_view() -> (TestApp, Rc<RefCell<Calls>>, Signal<Model>) {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let model_slot: Slot<Signal<Model>> = Rc::default();
    let (c, slot) = (Rc::clone(&calls), Rc::clone(&model_slot));
    let app = launcher_window(move |__scope: &mut RenderScope| {
        let mut model = Model::new();
        let all = entries();
        model.set_query("term", all.iter().map(|e| &e.entry));
        let model = Signal::new(model);
        *slot.borrow_mut() = Some(model);
        let index = Signal::new(None);
        let (q, k, p) = (Rc::clone(&c), Rc::clone(&c), Rc::clone(&c));
        let (content, _field) = view::view(
            __scope,
            model,
            index,
            move |text| q.borrow_mut().queries.push(text),
            move |key| {
                k.borrow_mut().keys.push(key.to_owned());
                matches!(key, "ArrowUp" | "ArrowDown" | "Enter" | "Escape")
            },
            Rc::new(move |input| p.borrow_mut().pointer.push(input)),
            || {},
        );
        content
    });
    let model = taken(&model_slot);
    (app, calls, model)
}

/// The app id of each result row, in order, as the model has them.
fn row_ids(model: Signal<Model>) -> Vec<DesktopId> {
    model.with(|m| {
        m.rows()[m.visible()]
            .iter()
            .map(|row| row.id.clone())
            .collect()
    })
}

#[test]
fn typing_and_backspace_reach_on_query() {
    let (mut app, calls, _model) = mounted_view();
    app.focus(".launcher-query");
    app.type_text("fi");
    app.press_key("Backspace");
    // The field holds the model's query, "term", and the caret is at its end.
    assert_eq!(calls.borrow().queries, ["termf", "termfi", "termf"]);
}

#[test]
fn the_four_keys_reach_on_key_and_a_letter_reaches_the_field() {
    let (mut app, calls, _model) = mounted_view();
    app.focus(".launcher-query");
    for key in ["ArrowUp", "ArrowDown", "Enter", "Escape"] {
        app.press_key(key);
    }
    app.press_key("x");
    let calls = calls.borrow();
    assert_eq!(
        calls
            .keys
            .iter()
            .filter(|key| key.as_str() != "x")
            .collect::<Vec<_>>(),
        ["ArrowUp", "ArrowDown", "Enter", "Escape"]
    );
    assert_eq!(
        calls.queries.last().map(String::as_str),
        Some("termx"),
        "the letter went into the field: {:?}",
        calls.queries
    );
}

#[test]
fn a_press_and_a_release_on_a_row_name_it() {
    let (mut app, calls, model) = mounted_view();
    let ids = row_ids(model);
    let rows = app.query_all(".launcher-row");
    assert_eq!(rows.len(), ids.len());
    app.click_on(&rows[1]);
    assert_eq!(
        calls.borrow().pointer,
        [
            Input::Press(Some(ids[1].clone())),
            Input::Release(Some(ids[1].clone()))
        ]
    );
}

#[test]
fn a_press_on_one_row_and_a_release_on_another_name_each() {
    let (mut app, calls, model) = mounted_view();
    let ids = row_ids(model);
    let rows = app.query_all(".launcher-row");
    app.press(&rows[0], MouseButton::Left);
    app.release(&rows[1], MouseButton::Left);
    // The model makes no click of these ([`deskrinch_launcher::Model`]'s
    // own tests); the view gives it both, each named by its row.
    assert_eq!(
        calls.borrow().pointer,
        [
            Input::Press(Some(ids[0].clone())),
            Input::Release(Some(ids[1].clone()))
        ]
    );
}

#[test]
fn a_press_on_the_padding_round_the_results_is_off_them() {
    let (mut app, calls, _model) = mounted_view();
    let launcher = app.bounds(".launcher");
    // The launcher's own padding: the nearest handler is the container's,
    // not a row's (rinch's dispatch stops at the nearest node with one).
    let (x, y) = (launcher.x + 2.0, launcher.y + 2.0);
    let field = app.bounds(".launcher-query");
    assert!(
        x < field.x || y < field.y,
        "the point is the field's: {field:?}"
    );
    for row in app.query_all(".launcher-row") {
        let row = app.bounds(&row);
        assert!(x < row.x || y < row.y, "the point is a row's: {row:?}");
    }
    app.click(x, y);
    assert_eq!(
        calls.borrow().pointer,
        [Input::Press(None), Input::Release(None)]
    );
}

// ── The wiring, `act` recorded ──────────────────────────────────────────

/// The wiring mounted, what it hands `act` recorded.
pub(super) struct Wired {
    pub(super) app: TestApp,
    pub(super) acts: Rc<RefCell<Vec<Action>>>,
    pub(super) model: Signal<Model>,
    pub(super) visibility: Arc<Visibility>,
}

/// How the wiring is mounted.
#[derive(Clone, Copy)]
pub(super) struct Mount {
    /// Whether the launcher is shown.
    pub(super) shown: bool,
    /// Whether its window was made shown, so the field is focused at the
    /// mount. The launcher makes it so when it starts shown.
    pub(super) made_shown: bool,
    /// Whether another focusable element (`.other`, a text field) is
    /// mounted before the launcher's content.
    pub(super) other: bool,
}

/// The wiring mounted shown or hidden as `shown` says, as the launcher
/// starts.
pub(super) fn wired(shown: bool) -> Wired {
    wired_as(Mount {
        shown,
        made_shown: shown,
        other: false,
    })
}

/// The wiring mounted as `mount` says, as the launcher draws it
/// ([`launcher_window`]).
pub(super) fn wired_as(mount: Mount) -> Wired {
    let acts = Rc::new(RefCell::new(Vec::new()));
    let visibility = Arc::new(Visibility::new(mount.shown));
    let model_slot: Slot<Signal<Model>> = Rc::default();
    let (a, v, slot) = (
        Rc::clone(&acts),
        Arc::clone(&visibility),
        Rc::clone(&model_slot),
    );
    let app = launcher_window(move |__scope: &mut RenderScope| {
        let model = Signal::new(Model::new());
        *slot.borrow_mut() = Some(model);
        let all = entries();
        let shown = v.signal();
        let content = wire(
            __scope,
            Wiring {
                model,
                entries: Memo::new(move || Arc::clone(&all)),
                index: Signal::new(None),
                visibility: v,
                shown,
                made_shown: mount.made_shown,
                act: Rc::new(move |action| a.borrow_mut().push(action)),
            },
        );
        if mount.other {
            rsx! {
                div {
                    input { class: "other", r#type: "text" }
                    {content}
                }
            }
        } else {
            content
        }
    });
    let model = taken(&model_slot);
    Wired {
        app,
        acts,
        model,
        visibility,
    }
}

impl Wired {
    /// What `act` was given that does something (not [`Action::Stay`]).
    fn done(&self) -> Vec<Action> {
        self.acts
            .borrow()
            .iter()
            .filter(|action| **action != Action::Stay)
            .cloned()
            .collect()
    }
}

#[test]
fn enter_launches_the_selected_application() {
    let mut wired = wired(true);
    wired.app.type_text("term");
    wired.app.press_key("ArrowDown");
    let selected = wired
        .model
        .with(|m| m.selected_row().map(|row| row.id.clone()));
    wired.app.press_key("Enter");
    assert_eq!(wired.done(), [Action::Launch(selected.expect("a result"))]);
}

#[test]
fn escape_hides() {
    let mut wired = wired(true);
    wired.app.type_text("term");
    wired.app.press_key("Escape");
    assert_eq!(wired.done(), [Action::Hide]);
}

#[test]
fn shown_with_the_keyboard_the_field_has_it() {
    let wired = wired(true);
    let field = wired.app.handle(".launcher-query");
    let focused = wired.app.focused().expect("an element has the keyboard");
    assert_eq!(
        focused.node_id(),
        field.node_id(),
        "the query field has the keyboard"
    );
}

#[test]
fn hiding_says_the_query_s_length_and_empties_it() {
    let log = Log::default();
    let wired = tracing::subscriber::with_default(log.clone(), || {
        let mut wired = wired(true);
        wired.app.type_text("term");
        wired.visibility.hide();
        wired.app.settle();
        wired
    });
    assert_eq!(wired.model.with(|m| m.query().to_owned()), "");
    assert!(log.lines().contains(&query_at_hide(4)), "{:?}", log.lines());
}

// ── The log, as the launcher writes it ──────────────────────────────────

/// A `tracing` subscriber keeping each event's message.
#[derive(Clone, Default)]
pub(super) struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    /// The messages so far.
    pub(super) fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// An event's `message` field, written out.
#[derive(Default)]
struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }
}

impl tracing::Subscriber for Log {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = Message::default();
        event.record(&mut message);
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
