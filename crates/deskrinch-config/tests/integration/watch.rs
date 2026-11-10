//! The watcher: reloads on a change, a creation, a removal and a save by
//! rename, and what it delivers.

use std::fs;
use std::time::Duration;

use deskrinch_config::{Diagnostic, Problem};

use crate::support::{Dirs, Panel, next_change, next_diagnostics, panel, write};

#[test]
fn reloads_when_a_file_changes() {
    let dirs = Dirs::new();
    write(&dirs.user(), "[panel]\nheight = 40\n");
    let (_watcher, initial, events) = dirs.watch();
    assert_eq!(initial, panel(40));
    write(&dirs.user(), "[panel]\nheight = 50\n");
    assert_eq!(next_change(&events), panel(50));
    write(&dirs.system(), "[panel]\nedge = 'bottom'\n");
    assert_eq!(
        next_change(&events),
        Panel {
            height: 50,
            edge: "bottom".into()
        }
    );
}

#[test]
fn reloads_when_a_file_and_its_directories_are_created() {
    let dirs = Dirs::new();
    let (_watcher, initial, events) = dirs.watch();
    assert_eq!(initial, Panel::default());
    // Neither `home/.config/deskrinch` nor its parents exist yet.
    write(&dirs.user(), "[panel]\nheight = 60\n");
    assert_eq!(next_change(&events), panel(60));
}

#[test]
fn reloads_when_a_file_is_removed() {
    let dirs = Dirs::new();
    write(&dirs.system(), "[panel]\nheight = 40\n");
    write(&dirs.user(), "[panel]\nheight = 50\n");
    let (_watcher, initial, events) = dirs.watch();
    assert_eq!(initial, panel(50));
    fs::remove_file(dirs.user()).unwrap();
    assert_eq!(next_change(&events), panel(40));
    fs::remove_dir_all(dirs.root.path().join("etc")).unwrap();
    assert_eq!(next_change(&events), Panel::default());
}

#[test]
fn reloads_when_an_editor_saves_by_renaming() {
    let dirs = Dirs::new();
    write(&dirs.user(), "[panel]\nheight = 40\n");
    let (_watcher, _, events) = dirs.watch();
    let temporary = dirs.user().with_file_name(".config.toml.tmp");
    fs::write(&temporary, "[panel]\nheight = 70\n").unwrap();
    fs::rename(&temporary, dirs.user()).unwrap();
    assert_eq!(next_change(&events), panel(70));
}

#[test]
fn follows_a_file_that_is_a_symbolic_link() {
    let dirs = Dirs::new();
    let dotfiles = dirs.root.path().join("dotfiles/deskrinch.toml");
    write(&dotfiles, "[panel]\nheight = 40\n");
    fs::create_dir_all(dirs.user().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&dotfiles, dirs.user()).unwrap();
    let (_watcher, initial, events) = dirs.watch();
    assert_eq!(initial, panel(40));
    write(&dotfiles, "[panel]\nheight = 45\n");
    assert_eq!(next_change(&events), panel(45));
}

#[test]
fn delivers_nothing_for_a_change_that_leaves_the_section_alone() {
    let dirs = Dirs::new();
    write(&dirs.user(), "[panel]\nheight = 40\n");
    let (_watcher, _, events) = dirs.watch();
    // Another component's section, a comment, a file beside it.
    write(
        &dirs.user(),
        "# mine\n[panel]\nheight = 40\n[launcher]\nterminal = 'foot'\n",
    );
    write(&dirs.user().with_file_name("notes.txt"), "unrelated");
    std::thread::sleep(Duration::from_millis(1500));
    // Had anything above been delivered, it would arrive before this.
    write(&dirs.user(), "[panel]\nheight = 41\n");
    assert_eq!(next_change(&events), panel(41));
}

#[test]
fn keeps_the_last_good_settings_when_a_file_breaks_on_reload() {
    let dirs = Dirs::new();
    write(&dirs.user(), "[panel]\nheight = 40\n");
    let (_watcher, _, events) = dirs.watch();

    write(&dirs.user(), "[panel]\nheight = 'tall'\n");
    let diagnostics = next_diagnostics(&events);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].file, dirs.user());
    assert_eq!(
        diagnostics[0].location.map(|l| (l.line, l.column)),
        Some((2, 10))
    );

    write(&dirs.user(), "[panel\n");
    let diagnostics = next_diagnostics(&events);
    assert!(matches!(diagnostics[0].problem, Problem::Syntax { .. }));

    // Fixed: the diagnostics clear, then the new settings arrive. Nothing
    // was delivered for the settings in between: they stayed at 40.
    write(&dirs.user(), "[panel]\nheight = 42\n");
    assert_eq!(next_diagnostics(&events), Vec::<Diagnostic>::new());
    assert_eq!(next_change(&events), panel(42));
}

#[test]
fn reports_an_unknown_key_added_on_reload() {
    let dirs = Dirs::new();
    write(&dirs.user(), "[panel]\nheight = 40\n");
    let (_watcher, _, events) = dirs.watch();
    write(&dirs.user(), "[panel]\nheight = 40\nheigth = 41\n");
    let diagnostics = next_diagnostics(&events);
    assert_eq!(
        diagnostics[0].to_string(),
        format!(
            "{}:3:1: warning: unknown key `panel.heigth`",
            dirs.user().display()
        )
    );
}
