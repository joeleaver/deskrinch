//! The taskbar on every bar, driven by rinch's harness with outputs played
//! (`TestApp::set_outputs`): each bar on its own output, the windows handed
//! to the bars by the follower's own step
//! ([`apply_window_change`](crate::widgets::apply_window_change)), and the
//! grace for a window not yet placed seen across the bars; and that step
//! hands the buttons over only when they change.
//!
//! The grace's length is the model's to pin (`deskrinch_taskbar::on_bar`'s
//! tests, with explicit instants): here the grace is judged by the real
//! time the window was heard, and only the timer that ends it is on the
//! harness's virtual clock. So these tests prove the wiring by order alone,
//! and assert nothing at a virtual time between the window's arrival and
//! the grace's end, where the real time the test takes would decide.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use deskrinch_component::handover;
use deskrinch_settings::panel::{Edge, Height};
use deskrinch_taskbar::{Actions, Change, GRACE, TaskWindow, Taskbar, WindowState};
use deskrinch_window_management::WindowId;
use rinch::outputs::{Output, OutputId, output_ids};
use rinch::prelude::*;
use rinch::testing::TestApp;

use super::bars;
use super::bars_tests::contexts;
use crate::surface::Placement;
use crate::widgets::{Buttons, Runs, apply_window_change};

/// The output named `name`, with id `id`, at 1920x1080 and scale 1.
fn screen(id: u64, name: &str) -> Output {
    Output {
        id: OutputId::from_raw(id),
        name: Some(name.to_owned()),
        position: (0.0, 0.0),
        size: (1920.0, 1080.0),
        scale: 1.0,
    }
}

/// A window titled `title`, on `outputs` (none: not yet placed).
fn window(id: u64, title: &str, outputs: &[&str]) -> TaskWindow<WindowId> {
    TaskWindow {
        id: WindowId::from_raw(id),
        title: title.to_owned(),
        app_id: String::new(),
        state: WindowState::empty(),
        parent: None,
        outputs: outputs
            .iter()
            .map(|&name| name.to_owned())
            .collect::<BTreeSet<_>>(),
    }
}

/// The bars on the outputs played, their windows fed as the follower feeds
/// them: the model behind its lock and the buttons' hand-over.
struct Bars {
    app: TestApp,
    model: Mutex<Taskbar<WindowId>>,
    buttons: deskrinch_component::Handover<Buttons>,
}

impl Bars {
    /// The bars, at the bottom, on `outputs`.
    fn on(outputs: Vec<Output>) -> Self {
        let (buttons, inbox) = handover();
        let mut app = TestApp::mount(move |__scope: &mut RenderScope| {
            contexts(
                inbox.into_signal(Arc::default()),
                Signal::new(Runs::default()),
            );
            let placement = Placement {
                edge: Edge::Bottom,
                height: Height::try_from(40).unwrap(),
            };
            bars(__scope, Memo::new(output_ids), Memo::new(move || placement))
        });
        app.set_outputs(outputs);
        let mut model = Taskbar::new();
        model.connected(Actions::all());
        Self {
            app,
            model: Mutex::new(model),
            buttons,
        }
    }

    /// `change`, heard now, through the follower's step, then the turns that
    /// bring it to the bars.
    fn hear(&mut self, change: Change<WindowId>) {
        apply_window_change(&self.model, change, Instant::now(), &self.buttons);
        self.app.settle();
    }

    /// The outputs whose bar shows a button titled `title`, by name.
    fn showing(&mut self, title: &str) -> Vec<String> {
        let mut names = Vec::new();
        for bar in self.app.windows() {
            let window = self.app.window(bar);
            let id = window
                .props()
                .wayland_layer
                .as_ref()
                .and_then(|layer| layer.output_id)
                .expect("a bar is placed on its output by id");
            let shown = window.query(".taskbar").map(|_| window.text(".taskbar"));
            if shown.is_some_and(|text| text.contains(title)) {
                let name = output(id).and_then(|o| o.name);
                names.push(name.expect("every played output is named"));
            }
        }
        names
    }
}

/// A window heard before the compositor placed it is on no bar, turn after
/// turn; placed inside its grace, it is on its own bar only, then and
/// through the whole grace and past it: its grace's timer was cleared, and
/// no bar ever showed it elsewhere.
#[test]
fn a_window_placed_inside_its_grace_is_only_ever_on_its_own_bar() {
    let mut bars = Bars::on(vec![screen(1, "DP-1"), screen(2, "DP-2")]);
    assert_eq!(bars.app.windows().len(), 2, "a bar on each output");
    bars.hear(Change::Added(window(1, "editor", &[])));
    assert_eq!(
        bars.showing("editor"),
        Vec::<String>::new(),
        "unplaced, on no bar"
    );
    for turn in 0..3 {
        bars.app.settle();
        assert_eq!(
            bars.showing("editor"),
            Vec::<String>::new(),
            "on no bar at turn {turn}"
        );
    }
    bars.hear(Change::Changed(window(1, "editor", &["DP-2"])));
    assert_eq!(
        bars.showing("editor"),
        ["DP-2"],
        "placed: on its own bar only"
    );
    let step = std::time::Duration::from_millis(10);
    let mut advanced = std::time::Duration::ZERO;
    while advanced <= GRACE * 2 {
        bars.app.advance_time(step);
        advanced += step;
        assert_eq!(
            bars.showing("editor"),
            ["DP-2"],
            "on its own bar only, {advanced:?} after it was placed"
        );
    }
}

/// A window never placed is on no bar until its grace is over, then on
/// every bar (the timer, armed for at most the grace, has fired whatever
/// the real time); placed afterwards, it is on its own bar only, at once.
#[test]
fn a_window_never_placed_is_on_every_bar_once_its_grace_is_over() {
    let mut bars = Bars::on(vec![screen(1, "DP-1"), screen(2, "DP-2")]);
    bars.hear(Change::Added(window(2, "notes", &[])));
    assert_eq!(
        bars.showing("notes"),
        Vec::<String>::new(),
        "unplaced, on no bar"
    );
    bars.app.advance_time(GRACE);
    assert_eq!(
        bars.showing("notes"),
        ["DP-1", "DP-2"],
        "its grace over: on every bar"
    );
    bars.hear(Change::Changed(window(2, "notes", &["DP-1"])));
    assert_eq!(
        bars.showing("notes"),
        ["DP-1"],
        "placed: on its own bar only"
    );
}

/// With one bar there is nowhere else to show a window: unplaced, it is on
/// that bar at once, with no virtual time advanced.
#[test]
fn with_one_bar_a_window_is_on_it_before_it_is_placed() {
    let mut bars = Bars::on(vec![screen(1, "DP-1")]);
    bars.hear(Change::Added(window(3, "mail", &[])));
    assert_eq!(bars.showing("mail"), ["DP-1"]);
}

/// The follower's step hands the buttons over only when they changed: a
/// change that leaves them as they were (a window the taskbar never had
/// closing) hands nothing over, so the UI's list is the very one it had,
/// and nothing reading it wakes.
#[test]
fn the_step_hands_the_buttons_over_only_when_they_change() {
    let (buttons, inbox) = handover();
    let before: Buttons = Arc::default();
    let initial = Arc::clone(&before);
    let held: Rc<Cell<Option<Signal<Buttons>>>> = Rc::default();
    let out = Rc::clone(&held);
    let mut app = TestApp::mount(move |__scope: &mut RenderScope| {
        out.set(Some(inbox.into_signal(initial)));
        rsx! { div {} }
    });
    let shown = held.take().expect("the mount made the signal");
    let mut model = Taskbar::new();
    model.connected(Actions::all());
    let model = Mutex::new(model);
    let gone = Change::Removed(WindowId::from_raw(4));
    apply_window_change(&model, gone, Instant::now(), &buttons);
    app.settle();
    assert!(
        Arc::ptr_eq(&shown.get(), &before),
        "no change, yet the buttons were handed over"
    );
    let added = Change::Added(window(4, "editor", &["DP-1"]));
    apply_window_change(&model, added, Instant::now(), &buttons);
    app.settle();
    assert_eq!(shown.get().len(), 1, "a window added is handed over");
}
