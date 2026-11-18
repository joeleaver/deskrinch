//! The bars' windows, driven by rinch's harness for the windows a
//! component opens (`rinch::testing`): [`bars`] mounted with an output list
//! and a placement the test owns, each bar's window opened after the turn
//! that built its row, as the shell opens it, and its requests read in the
//! order made.
//!
//! What the compositor tests cannot see or order: a bar's opening props, a
//! settings change after the turn that builds a row and before its window
//! opens, an output going in the turn its placement changes, and a bar's
//! window closed by the compositor while its output stays.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use deskrinch_settings::panel::{Edge, Height};
use rinch::OutputId;
use rinch::prelude::*;
use rinch::testing::{TestApp, WindowRequest};

use super::bars;
use crate::surface::{self, Placement};
use crate::widgets::{AppIcons, ClockText, FollowedWindows, FollowedWorkspaces};

fn at(edge: Edge, height: u32) -> Placement {
    Placement {
        edge,
        height: Height::try_from(height).unwrap(),
    }
}

fn output(raw: u64) -> OutputId {
    OutputId::from_raw(raw)
}

/// The signals a test owns: the outputs and the placement [`bars`] follow.
#[derive(Clone, Copy)]
struct Owned {
    outputs: Signal<Vec<OutputId>>,
    placement: Signal<Placement>,
}

/// [`bars`] on `outputs` at `placement`, with the contexts the bar's
/// widgets read (inert: no compositor connection, no applications, a fixed
/// clock face). Returns the app, settled, and the signals.
fn mount(outputs: Vec<OutputId>, placement: Placement) -> (TestApp, Owned) {
    let owned: Rc<Cell<Option<Owned>>> = Rc::default();
    let out = Rc::clone(&owned);
    let mut app = TestApp::mount(move |__scope: &mut RenderScope| {
        create_context(AppIcons {
            applications: Signal::new(Arc::default()),
            index: Signal::new(None),
        });
        create_context(Rc::new(FollowedWindows::new().0));
        create_context(Rc::new(FollowedWorkspaces::new().0));
        create_context(Signal::new(crate::widgets::Buttons::default()));
        create_context(Signal::new(crate::widgets::Runs::default()));
        create_context(ClockText::fixed(Signal::new("12:00".to_owned())));
        let signals = Owned {
            outputs: Signal::new(outputs),
            placement: Signal::new(placement),
        };
        out.set(Some(signals));
        bars(
            __scope,
            Memo::new(move || signals.outputs.get()),
            Memo::new(move || signals.placement.get()),
        )
    });
    app.settle();
    let signals = owned.get().expect("the signals, made at the mount");
    (app, signals)
}

/// What a bar placed at `placement` on `output` asks of its window to move
/// there, its width the one it opened with.
fn moved_to(placement: Placement, output: OutputId) -> [WindowRequest; 2] {
    let surface = surface::bar_surface(placement, output);
    [
        WindowRequest::SetSize(f64::from(surface::FIRST_WIDTH), f64::from(surface.height)),
        WindowRequest::SetLayer(Box::new(surface.layer)),
    ]
}

/// The line a bar on `output` says when it is placed at `placement`: the
/// output by its id, which has no name in the harness.
fn placed(output: OutputId, placement: Placement) -> String {
    deskrinch_panel::placed(&format!("{output:?}"), placement.edge, placement.height)
}

/// Each bar opens on its own output: two outputs, so that a bar opened on
/// the first whatever its row would not pass for the second.
#[test]
fn a_bar_opens_borderless_on_its_output_at_the_placement() {
    let placement = at(Edge::Bottom, 40);
    let (mut app, _) = mount(vec![output(1), output(2)], placement);
    let windows = app.windows();
    assert_eq!(windows.len(), 2);
    for (window, id) in windows.into_iter().zip([output(1), output(2)]) {
        let mut window = app.window(window);
        let props = window.props().clone();
        let opened = surface::bar_surface(placement, id);
        assert_eq!(props.title, surface::TITLE);
        assert_eq!(props.width, surface::FIRST_WIDTH);
        assert_eq!(props.height, 40);
        assert!(props.borderless);
        assert_eq!(props.app_id.as_deref(), Some(surface::NAMESPACE));
        assert_eq!(
            props.wayland_layer,
            Some(opened.layer),
            "the bar for {id:?} opened on its own output"
        );
        // Opened as it is to be: nothing more is asked of it.
        assert_eq!(window.take_requests(), []);
    }
}

/// The compositor closes a bar's window while its output stays (never seen
/// under the three compositors): as `surface.rs` says, that output has no
/// bar until it goes and comes back, when it gets exactly one new one.
#[test]
fn a_bar_closed_by_the_compositor_comes_back_with_its_output() {
    let (mut app, owned) = mount(vec![output(1), output(2)], at(Edge::Bottom, 40));
    let windows = app.windows();
    assert_eq!(windows.len(), 2);
    app.window(windows[1]).close_by_system();
    app.settle();
    assert_eq!(app.windows(), [windows[0]]);
    owned.outputs.set(vec![output(1)]);
    app.settle();
    assert_eq!(app.windows(), [windows[0]]);
    owned.outputs.set(vec![output(1), output(2)]);
    app.settle();
    let now = app.windows();
    assert_eq!(now.len(), 2);
    assert_eq!(now[0], windows[0]);
    assert_ne!(now[1], windows[1], "the closed window again, not a new one");
}

/// The settings change after the turn that builds a row and before its
/// window opens: the outputs set from the test's body build the row with
/// the first settings, its window pending until the requests are served
/// (`settle`); the second settings come in between. The window opens with
/// the first, and is asked once for the second's size and layer, in that
/// order, and nothing more.
#[test]
fn a_settings_change_before_a_rows_window_opens_is_sent_once() {
    let first = at(Edge::Bottom, 40);
    let second = at(Edge::Top, 56);
    let (mut app, owned) = mount(vec![], first);
    owned.outputs.set(vec![output(1)]);
    assert_eq!(
        app.windows(),
        [],
        "the window opened before the requests were served"
    );
    owned.placement.set(second);
    app.settle();
    let windows = app.windows();
    assert_eq!(windows.len(), 1);
    let mut window = app.window(windows[0]);
    let opened = surface::bar_surface(first, output(1));
    assert_eq!(
        window.props().height,
        opened.height,
        "not opened with the first settings"
    );
    assert_eq!(window.props().wayland_layer, Some(opened.layer));
    assert_eq!(window.take_requests(), moved_to(second, output(1)));
    app.settle();
    assert_eq!(app.window(windows[0]).take_requests(), []);
}

/// An output goes in the turn the placement changes: the other bar moves
/// and says so; the bar of the output that went asks nothing of its
/// closing window and says nothing; nothing is said at warn or error. The
/// first output goes and the second stays, so that a bar reading the first
/// output's presence for its own would not pass.
#[test]
fn an_output_going_as_the_settings_change_moves_only_the_bar_that_stays() {
    let first = at(Edge::Bottom, 40);
    let second = at(Edge::Top, 56);
    let (mut app, owned) = mount(vec![output(1), output(2)], first);
    let windows = app.windows();
    assert_eq!(windows.len(), 2);
    drop(app.take_window_requests());
    let log = deskrinch_test_log::capture();
    owned.outputs.set(vec![output(2)]);
    owned.placement.set(second);
    app.settle();
    assert_eq!(app.windows(), [windows[1]]);
    assert_eq!(
        app.window(windows[1]).take_requests(),
        moved_to(second, output(2))
    );
    let asked: Vec<WindowRequest> = app
        .take_window_requests()
        .into_iter()
        .filter(|request| matches!(request, WindowRequest::OtherWindow { .. }))
        .collect();
    assert_eq!(asked, [], "asked of the window that went");
    let placed_lines: Vec<String> = log
        .lines()
        .into_iter()
        .filter(|line| line.contains(" is placed along the "))
        .collect();
    assert_eq!(placed_lines, [placed(output(2), second)]);
    assert_eq!(log.warnings_and_errors(), Vec::<String>::new());
}
