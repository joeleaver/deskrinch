use super::*;

fn window(id: u32, state: WindowState, parent: Option<u32>) -> TaskWindow<u32> {
    TaskWindow {
        id,
        title: format!("window {id}"),
        app_id: format!("org.example.App{id}"),
        state,
        parent,
    }
}

fn plain(id: u32) -> TaskWindow<u32> {
    window(id, WindowState::empty(), None)
}

/// A taskbar that can do everything, with `windows` added in order.
fn taskbar(windows: impl IntoIterator<Item = TaskWindow<u32>>) -> Taskbar<u32> {
    let mut taskbar = Taskbar::new();
    for window in windows {
        taskbar.input(Input::Change(Change::Added(window)));
    }
    taskbar.connected(Actions::all());
    taskbar
}

fn ids(taskbar: &Taskbar<u32>) -> Vec<u32> {
    taskbar.buttons().iter().map(|button| button.id).collect()
}

/// What `answer` asks to be done, if anything.
fn performed(answer: Answer<u32>) -> Option<(u32, Vec<Action>)> {
    match answer {
        Answer::Perform(id, actions) => Some((id, actions)),
        Answer::Stay => None,
        Answer::Ended(reason) => panic!("ended: {reason}"),
    }
}

fn click(taskbar: &mut Taskbar<u32>, id: u32, button: PointerButton) -> Option<(u32, Vec<Action>)> {
    taskbar.input(Input::Press(Some(id), button));
    performed(taskbar.input(Input::Release(Some(id), button)))
}

fn change(taskbar: &mut Taskbar<u32>, change: Change<u32>) -> Answer<u32> {
    taskbar.input(Input::Change(change))
}

#[test]
fn every_window_without_a_parent_or_a_wish_to_be_left_off_has_a_button() {
    let taskbar = taskbar([
        plain(1),
        window(2, WindowState::SKIP_TASKBAR, None),
        window(3, WindowState::empty(), Some(1)),
        plain(4),
    ]);
    assert_eq!(ids(&taskbar), [1, 4]);
}

#[test]
fn the_buttons_are_in_the_order_their_windows_were_added() {
    let mut taskbar = taskbar([plain(3), plain(1)]);
    change(&mut taskbar, Change::Added(plain(2)));
    assert_eq!(ids(&taskbar), [3, 1, 2]);
    // A change keeps a window in its place.
    let mut retitled = plain(3);
    retitled.title = "renamed".into();
    change(&mut taskbar, Change::Changed(retitled));
    assert_eq!(ids(&taskbar), [3, 1, 2]);
    assert_eq!(taskbar.buttons()[0].title, "renamed");
}

#[test]
fn a_removed_window_loses_its_button() {
    let mut taskbar = taskbar([plain(1), plain(2)]);
    change(&mut taskbar, Change::Removed(1));
    assert_eq!(ids(&taskbar), [2]);
    change(&mut taskbar, Change::Removed(7));
    assert_eq!(ids(&taskbar), [2], "an unknown window changes nothing");
}

#[test]
fn a_change_can_take_a_button_away_and_give_it_back_in_its_place() {
    let mut taskbar = taskbar([plain(1), plain(2), plain(3)]);
    change(
        &mut taskbar,
        Change::Changed(window(2, WindowState::SKIP_TASKBAR, None)),
    );
    assert_eq!(ids(&taskbar), [1, 3]);
    change(&mut taskbar, Change::Changed(plain(2)));
    assert_eq!(ids(&taskbar), [1, 2, 3]);
}

#[test]
fn a_dialog_whose_parent_is_gone_has_a_button_of_its_own() {
    let mut taskbar = taskbar([plain(1), window(2, WindowState::empty(), Some(1))]);
    assert_eq!(ids(&taskbar), [1]);
    change(&mut taskbar, Change::Removed(1));
    assert_eq!(ids(&taskbar), [2]);
    let orphan = self::taskbar([window(5, WindowState::empty(), Some(99))]);
    assert_eq!(ids(&orphan), [5], "a parent never known");
}

#[test]
fn a_button_is_active_or_asks_for_attention_through_its_dialogs() {
    let taskbar = taskbar([
        plain(1),
        window(2, WindowState::empty(), Some(1)),
        window(3, WindowState::ACTIVE, Some(2)),
        plain(4),
        window(5, WindowState::DEMANDS_ATTENTION, Some(4)),
    ]);
    let buttons = taskbar.buttons();
    assert_eq!(ids(&taskbar), [1, 4]);
    assert!(buttons[0].active, "a dialog of a dialog is active");
    assert!(!buttons[0].attention);
    assert!(!buttons[1].active);
    assert!(buttons[1].attention, "its dialog asks for attention");
}

#[test]
fn a_minimized_window_is_dimmed_and_its_own_state_only() {
    let taskbar = taskbar([
        window(1, WindowState::MINIMIZED, None),
        window(2, WindowState::MINIMIZED, Some(1)),
        plain(3),
    ]);
    let buttons = taskbar.buttons();
    assert!(buttons[0].minimized);
    assert!(!buttons[1].minimized);
}

#[test]
fn a_click_activates_a_window() {
    let mut taskbar = taskbar([plain(1), plain(2)]);
    assert_eq!(
        click(&mut taskbar, 2, PointerButton::Primary),
        Some((2, vec![Action::Activate]))
    );
}

#[test]
fn a_click_on_the_active_button_minimizes_its_own_window() {
    let mut taskbar = taskbar([plain(1), window(2, WindowState::ACTIVE, Some(1))]);
    assert_eq!(
        click(&mut taskbar, 1, PointerButton::Primary),
        Some((1, vec![Action::Minimize])),
        "active through its dialog; the window itself is minimized"
    );
}

#[test]
fn without_minimize_a_click_on_the_active_button_does_nothing() {
    let mut taskbar = taskbar([window(1, WindowState::ACTIVE, None)]);
    taskbar.connected(Actions::ACTIVATE | Actions::CLOSE);
    assert_eq!(click(&mut taskbar, 1, PointerButton::Primary), None);
}

#[test]
fn a_click_on_a_minimized_window_restores_then_activates_it() {
    let mut taskbar = taskbar([window(1, WindowState::MINIMIZED, None)]);
    assert_eq!(
        click(&mut taskbar, 1, PointerButton::Primary),
        Some((1, vec![Action::Restore, Action::Activate]))
    );
    taskbar.connected(Actions::ACTIVATE | Actions::MINIMIZE);
    assert_eq!(
        click(&mut taskbar, 1, PointerButton::Primary),
        Some((1, vec![Action::Activate])),
        "activate alone where restore is not offered"
    );
}

#[test]
fn a_middle_click_closes_where_closing_is_offered() {
    let mut taskbar = taskbar([plain(1)]);
    assert_eq!(
        click(&mut taskbar, 1, PointerButton::Middle),
        Some((1, vec![Action::Close]))
    );
    taskbar.connected(Actions::ACTIVATE);
    assert_eq!(click(&mut taskbar, 1, PointerButton::Middle), None);
}

#[test]
fn a_press_and_a_release_on_different_buttons_do_nothing() {
    let mut taskbar = taskbar([plain(1), plain(2)]);
    taskbar.input(Input::Press(Some(1), PointerButton::Primary));
    assert_eq!(
        performed(taskbar.input(Input::Release(Some(2), PointerButton::Primary))),
        None
    );
    taskbar.input(Input::Press(Some(1), PointerButton::Primary));
    assert_eq!(
        performed(taskbar.input(Input::Release(None, PointerButton::Primary))),
        None
    );
    taskbar.input(Input::Press(Some(1), PointerButton::Primary));
    assert_eq!(
        performed(taskbar.input(Input::Release(Some(1), PointerButton::Middle))),
        None,
        "another button's release"
    );
}

#[test]
fn a_click_on_a_window_that_went_meanwhile_does_nothing() {
    let mut taskbar = taskbar([plain(1)]);
    taskbar.input(Input::Press(Some(1), PointerButton::Primary));
    change(&mut taskbar, Change::Removed(1));
    assert_eq!(
        performed(taskbar.input(Input::Release(Some(1), PointerButton::Primary))),
        None
    );
}

#[test]
fn before_the_connection_says_what_it_can_do_nothing_is_asked() {
    let mut taskbar = Taskbar::new();
    change(&mut taskbar, Change::Added(plain(1)));
    assert_eq!(click(&mut taskbar, 1, PointerButton::Primary), None);
}

#[test]
fn the_end_empties_it_once_and_nothing_follows() {
    let mut taskbar = taskbar([plain(1)]);
    assert!(matches!(
        change(&mut taskbar, Change::Ended(EndReason::Finished)),
        Answer::Ended(EndReason::Finished)
    ));
    assert_eq!(ids(&taskbar), Vec::<u32>::new());
    assert!(
        matches!(
            change(&mut taskbar, Change::Ended(EndReason::Finished)),
            Answer::Stay
        ),
        "said once"
    );
    change(&mut taskbar, Change::Added(plain(2)));
    assert_eq!(ids(&taskbar), Vec::<u32>::new(), "nothing after the end");
}

#[test]
fn a_taskbar_that_could_not_connect_shows_and_does_nothing() {
    let mut taskbar = Taskbar::<u32>::unavailable();
    change(&mut taskbar, Change::Added(plain(1)));
    assert_eq!(ids(&taskbar), Vec::<u32>::new());
}
