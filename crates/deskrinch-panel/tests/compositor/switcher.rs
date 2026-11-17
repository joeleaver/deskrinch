//! The workspace switcher against real compositors: a button for each
//! workspace at the bar's start, the current one marked, a click switching,
//! a change from outside followed; and, under sway, which offers no
//! workspace protocol, no switcher at all and one line saying why.
//!
//! The buttons show positions, not colours a test could find, so they are
//! found by their mark: each workspace is made current in turn, and the
//! mark's box (in a row near the top of the panel, above the text, where
//! only a button's background differs from the bar's) is that workspace's
//! button. The test's own `deskrinch-workspaces` connection makes a
//! workspace current from outside the panel and reads what the compositor
//! did; KWin's own D-Bus interface (`deskrinch-test-desktops`) adds and
//! removes desktops, which the protocols do not offer.
//!
//! Each change of the current workspace draws once: every switch has a
//! frame, and a switch to every workspace is seen clean (exactly one frame,
//! which repaints and lays nothing out), the switches cycling until each
//! has, in contiguous windows; other frames are the panel's other followers
//! (the clock's minute). See [`Pager::sweep`] for the rule and its limit.

use deskrinch_test_client::Compositor;
use deskrinch_test_desktops as kwin;
use deskrinch_test_input::{Button, Input};
use deskrinch_test_screen::{Image, Screen, still_gap};
use deskrinch_test_wait::{Deadline, PATIENCE, wait_for};
use deskrinch_workspaces::{
    Action, Workspace, WorkspaceManager, WorkspaceState, Workspaces, connect,
};

use crate::frames;
use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};
use crate::taskbar::near;

/// The panel's height, in logical pixels.
const HEIGHT: u32 = 40;
/// The workspaces every test has: labwc's sessions configure four, and the
/// KWin tests make four.
const COUNT: usize = 4;
/// A switcher button's width, in logical pixels: the panel's.
const BUTTON: u32 = 28;
/// What the panel says where there are no workspaces to show.
const ABSENT: &str = "no workspace protocol offered";

/// A button's box on the screen: its first and last column, in pixels.
type Columns = (u32, u32);

/// The panel at the bottom, the screen it is on, the pointer, and a
/// workspaces connection of the test's own; `None` under sway.
struct Pager {
    input: Input,
    screen: Screen,
    panel: Panel,
    output: String,
    scale: u32,
    band: Band,
    workspaces: Option<WorkspaceManager>,
}

impl Pager {
    /// The pointer, the workspaces (at least [`COUNT`]), then the panel in
    /// the dark scheme, once it is on the screen.
    fn start() -> Self {
        in_nested_session();
        let input = Input::connect().unwrap();
        let workspaces = match Compositor::current() {
            Compositor::Sway => None,
            Compositor::Kwin => {
                let _first = kwin::set_up(COUNT);
                Some(connect(|_| {}).unwrap())
            }
            Compositor::Labwc => Some(connect(|_| {}).unwrap()),
        };
        write_settings(&format!(
            "[general]\ncolor-scheme = \"dark\"\n{}",
            panel_table(Edge::Bottom, HEIGHT)
        ));
        let mut screen = Screen::connect().unwrap();
        let before = Pictures::still(&mut screen);
        let panel = Panel::start();
        let scale = scale_of(&screen.outputs()[0]);
        let band = Band::of(Edge::Bottom, HEIGHT, scale);
        let (output, _) = wait_band(&mut screen, &before, band);
        let mut pager = Self {
            input,
            screen,
            panel,
            output,
            scale,
            band,
            workspaces,
        };
        pager.point_off();
        pager
    }

    /// The workspaces with a button, in the switcher's order: the
    /// compositor's layout where it gives one, else its order.
    fn shown(&self) -> Vec<Workspace> {
        let manager = self.workspaces.as_ref().expect("a workspace protocol");
        let mut shown: Vec<Workspace> = manager
            .workspaces()
            .into_iter()
            .filter(|w| !w.state.contains(WorkspaceState::HIDDEN))
            .collect();
        if shown.iter().all(|w| !w.coordinates.is_empty()) {
            shown.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));
        }
        shown
    }

    /// Makes `workspace` current from outside the panel, and waits until
    /// the compositor says it is.
    fn activate(&self, workspace: &Workspace) {
        let manager = self.workspaces.as_ref().expect("a workspace protocol");
        manager.perform(workspace.id, Action::Activate).unwrap();
        self.wait_current(workspace);
    }

    /// Waits until the compositor says `workspace` is current.
    fn wait_current(&self, workspace: &Workspace) {
        let manager = self.workspaces.as_ref().expect("a workspace protocol");
        wait_for(&format!("{} current", workspace.name), || {
            let listed = manager.workspaces();
            if listed
                .iter()
                .any(|w| w.id == workspace.id && w.state.contains(WorkspaceState::ACTIVE))
            {
                Ok(())
            } else {
                Err(listed)
            }
        });
    }

    /// The mark's box in `image`: the first run of pixels unlike the bar's
    /// in a row two pixels below the panel's top, in the bar's first third.
    fn mark(&self, image: &Image) -> Option<Columns> {
        let y = self.band.rows_in(image.height()).start + 2 * self.scale;
        let bar = image.pixel(1, y)?;
        let unlike = |x: u32| image.pixel(x, y).is_some_and(|p| !near(p, bar));
        let first = (0..image.width() / 3).find(|&x| unlike(x))?;
        let last = (first..image.width() / 3)
            .take_while(|&x| unlike(x))
            .last()
            .unwrap_or(first);
        Some((first, last))
    }

    /// Captures the panel's output until its mark is what `accept` takes
    /// in two captures [`still_gap`] apart, alike: the mark settled where it is
    /// wanted, not caught on its way. Returns it.
    fn wait_mark(
        &mut self,
        what: &str,
        accept: impl Fn(Option<Columns>) -> bool,
    ) -> Option<Columns> {
        let output = self.output.clone();
        wait_for(what, || {
            let image = self.screen.capture(&output).unwrap();
            let before = self.mark(&image);
            if !accept(before) {
                return Err(before);
            }
            std::thread::sleep(still_gap());
            let image = self.screen.capture(&output).unwrap();
            let after = self.mark(&image);
            if after == before {
                Ok(after)
            } else {
                Err(after)
            }
        })
    }

    /// Moves the pointer off the panel, to the middle of its output.
    fn point_off(&mut self) {
        let image = self.screen.capture(&self.output.clone()).unwrap();
        let scale = f64::from(self.scale);
        let x = f64::from(image.width()) / 2.0 / scale;
        let y = f64::from(image.height()) / 2.0 / scale;
        self.input.move_pointer(&self.output, x, y).unwrap();
    }

    /// Moves the pointer onto the middle of the button at `columns`.
    fn point_at(&mut self, columns: Columns) {
        let image = self.screen.capture(&self.output.clone()).unwrap();
        let rows = self.band.rows_in(image.height());
        let scale = f64::from(self.scale);
        let x = f64::from(columns.0 + columns.1 + 1) / 2.0 / scale;
        let y = f64::from(rows.start + rows.end) / 2.0 / scale;
        self.input.move_pointer(&self.output, x, y).unwrap();
    }

    /// Clicks the middle of the button at `columns`.
    fn click(&mut self, columns: Columns) {
        self.point_at(columns);
        self.input.click(Button::PRIMARY).unwrap();
    }

    /// Under sway: no switcher drawn, and the panel says why.
    fn assert_absent(&mut self) {
        assert!(
            self.panel
                .watch()
                .wait_for_line(PATIENCE, |line| line.contains(ABSENT)),
            "the panel did not say {ABSENT:?}; it logged:\n{}",
            self.panel.log()
        );
        let image = self.screen.capture(&self.output.clone()).unwrap();
        assert_eq!(
            self.mark(&image),
            None,
            "something is drawn at the bar's start"
        );
    }

    /// Every shown workspace made current in turn, from the first, cycle
    /// after cycle, until a switch to each has been seen clean: their
    /// buttons' boxes, in order. The last is current afterwards.
    ///
    /// The last workspace is made current once before the first cycle, and
    /// the panel's log marked once the screen keeps still; from there the
    /// steps' windows are contiguous, each beginning where the previous
    /// step's read stopped, so no frame between two switches goes unread.
    ///
    /// A switch is clean when its window holds exactly one frame, and that
    /// frame repainted and laid nothing out: the mark moved, and nothing
    /// else drew. A window with more frames, or one frame that laid out, is
    /// crowded: the clock's minute, a title or a late arrival drew there
    /// too, or batched into the switch's frame. Each workspace is kept
    /// apart, and the cycles go on until every one has had a clean switch,
    /// so a switch that relays out for one workspace fails, naming it, at
    /// the patience, with the frames of its switches. The first cycle
    /// counts: a late frame of the switch made before it lands in the first
    /// switch's window, which only puts off that workspace's clean switch to
    /// a later cycle.
    ///
    /// The rule's limit: a frame drawn later than its own step is read in
    /// the next step's window, and crowds the workspace switched to there
    /// instead; and work deferred by longer than a step (a switch, the mark
    /// moving, and the screen keeping still for the stability window) is
    /// not told apart from the next switch's.
    fn sweep(&mut self) -> Vec<Columns> {
        let shown = self.shown();
        assert!(shown.len() >= COUNT, "{shown:#?}");
        // From the last, so that making the first current is a change.
        self.activate(&shown[shown.len() - 1]);
        let mut previous = self.wait_mark("the last workspace's mark", |m| m.is_some());
        let mut from = frames::step(&self.panel);
        let deadline = Deadline::patience();
        let mut boxes: Vec<Option<Columns>> = vec![None; shown.len()];
        let mut clean = vec![false; shown.len()];
        let mut crowded: Vec<(String, Vec<String>)> = Vec::new();
        loop {
            for (index, workspace) in shown.iter().enumerate() {
                self.activate(workspace);
                let mark = self.wait_mark(&format!("{}'s mark", workspace.name), |m| {
                    m.is_some() && (m != previous || shown.len() == 1)
                });
                let (drawn, end) = frames::drawn_since(&self.panel, from);
                from = end;
                assert!(
                    !drawn.is_empty(),
                    "{}: the switch drew nothing",
                    workspace.name
                );
                match drawn.as_slice() {
                    [frame] if frames::is_paint_only(frame) => clean[index] = true,
                    _ => crowded.push((workspace.name.clone(), drawn)),
                }
                boxes[index].get_or_insert(mark.expect("a mark"));
                previous = mark;
            }
            if clean.iter().all(|seen| *seen) {
                return boxes.into_iter().flatten().collect();
            }
            let never: Vec<&str> = shown
                .iter()
                .zip(&clean)
                .filter(|(_, seen)| !**seen)
                .map(|(workspace, _)| workspace.name.as_str())
                .collect();
            assert!(
                !deadline.passed(),
                "no clean switch to {never:?}: every one drew more than one frame, or laid \
                 out; the crowded switches: {crowded:#?}"
            );
        }
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_workspaces_are_listed_in_order_with_the_current_one_marked() {
    let mut pager = Pager::start();
    if pager.workspaces.is_none() {
        pager.assert_absent();
        return;
    }
    let boxes = pager.sweep();
    let width = BUTTON * pager.scale;
    for (index, &(first, last)) in boxes.iter().enumerate() {
        let drawn = last - first + 1;
        assert!(
            drawn.abs_diff(width) <= pager.scale,
            "button {index} is {drawn} pixels wide, not {width}: {boxes:?}"
        );
        if index > 0 {
            assert!(first > boxes[index - 1].1, "not in order: {boxes:?}");
        }
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_click_switches_to_its_workspace() {
    let mut pager = Pager::start();
    if pager.workspaces.is_none() {
        pager.assert_absent();
        return;
    }
    let boxes = pager.sweep();
    let first = pager.shown().remove(0);
    pager.click(boxes[0]);
    pager.wait_current(&first);
    let mark = pager.wait_mark("the first workspace's mark", |m| m == Some(boxes[0]));
    assert_eq!(mark, Some(boxes[0]));
    pager.point_off();
}

/// The pointer onto a button that is not the current one marks it (the
/// bar's shared `:hover` rule), and off the panel the mark goes, round
/// after round until a round after the first is clean: the taskbar's hover
/// rule, for the same reasons ([`frames::until_clean_round`],
/// [`crate::taskbar_hover`]).
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_button_under_the_pointer_is_marked_until_it_leaves() {
    let mut pager = Pager::start();
    if pager.workspaces.is_none() {
        pager.assert_absent();
        return;
    }
    let boxes = pager.sweep();
    // The last is current; the first is not, and is the first mark from
    // the left once the pointer is on it.
    let first = boxes[0];
    let last = boxes[boxes.len() - 1];
    frames::until_clean_round(
        &mut pager,
        |pager| &pager.panel,
        |pager| {
            pager.point_at(first);
            let mark = pager.wait_mark("the first button marked under the pointer", |m| {
                m == Some(first)
            });
            assert_eq!(mark, Some(first));
        },
        |pager| {
            pager.point_off();
            let mark = pager.wait_mark("the first button's mark gone", |m| m == Some(last));
            assert_eq!(
                mark,
                Some(last),
                "only the current workspace's mark is left"
            );
        },
    );
}

/// KWin alone can add and remove desktops while it runs. Under labwc, whose
/// workspaces its configuration fixes, the switcher shows exactly the
/// session's four, as every compositor-specific test of the panel asserts
/// what is true where it runs; sway has none to show.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_desktop_added_or_removed_is_followed() {
    let mut pager = Pager::start();
    match Compositor::current() {
        Compositor::Sway => {
            pager.assert_absent();
            return;
        }
        Compositor::Labwc => {
            let boxes = pager.sweep();
            assert_eq!(boxes.len(), COUNT, "labwc's fixed workspaces: {boxes:?}");
            return;
        }
        Compositor::Kwin => {}
    }
    let before = pager.sweep();
    let name = format!("Extra {}", std::process::id());
    kwin::create(&name);
    let added = wait_for("the added desktop", || {
        let shown = pager.shown();
        shown.iter().find(|w| w.name == name).cloned().ok_or(shown)
    });
    pager.activate(&added);
    let last = before[before.len() - 1];
    let mark = pager.wait_mark("the added desktop's mark", |m| {
        m.is_some_and(|(first, _)| first > last.1)
    });
    assert!(
        mark.is_some_and(|(first, _)| first > last.1),
        "{mark:?} after {before:?}"
    );
    kwin::remove(&name);
    wait_for("the desktop removed", || {
        let shown = pager.shown();
        if shown.iter().any(|w| w.name == name) {
            Err(shown)
        } else {
            Ok(())
        }
    });
    let after = pager.sweep();
    assert_eq!(
        after, before,
        "the buttons are as before the desktop was added"
    );
}
