//! The panel as `deskrinch-shell` describes it, for every starter of the
//! shell: what it must be granted is what the panel uses.

#[test]
fn the_shell_grants_the_panel_the_window_list_it_follows() {
    let panel = deskrinch_shell::COMPONENTS
        .iter()
        .find(|component| component.package == env!("CARGO_PKG_NAME"))
        .expect("the panel is one of the shell's components");
    assert_eq!(panel, &deskrinch_shell::PANEL);
    assert_eq!(
        panel.wayland_grants,
        [deskrinch_window_management::KWIN_INTERFACE]
    );
}
