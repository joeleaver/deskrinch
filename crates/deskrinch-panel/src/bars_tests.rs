//! The bars' windows, driven by rinch's harness for the windows a
//! component opens (`rinch::testing`): [`bars`] mounted with an output list
//! and a placement the test owns, each bar's window opened after the turn
//! that built its row, as the shell opens it, and its requests read in the
//! order made.
//!
//! What the compositor tests cannot see: a bar's opening props, and its
//! window closed by the compositor while its output stays.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use deskrinch_settings::panel::{Edge, Height};
use rinch::OutputId;
use rinch::prelude::*;
use rinch::testing::TestApp;

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

#[test]
fn a_bar_opens_borderless_on_its_output_at_the_placement() {
    let placement = at(Edge::Bottom, 40);
    let (mut app, _) = mount(vec![output(1)], placement);
    let windows = app.windows();
    assert_eq!(windows.len(), 1);
    let mut window = app.window(windows[0]);
    let props = window.props().clone();
    let opened = surface::bar_surface(placement, output(1));
    assert_eq!(props.title, surface::TITLE);
    assert_eq!(props.width, surface::FIRST_WIDTH);
    assert_eq!(props.height, 40);
    assert!(props.borderless);
    assert_eq!(props.app_id.as_deref(), Some(surface::NAMESPACE));
    assert_eq!(props.wayland_layer, Some(opened.layer));
    // Opened as it is to be: nothing more is asked of it.
    assert_eq!(window.take_requests(), []);
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
