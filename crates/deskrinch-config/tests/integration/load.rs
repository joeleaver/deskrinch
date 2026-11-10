//! Loading the settings once from real files.

use std::fs;

use deskrinch_config::{Diagnostic, Problem, Section, load};

use crate::support::{Dirs, Panel, panel, write};

#[test]
fn loads_the_layers_from_files() {
    let dirs = Dirs::new();
    write(&dirs.system(), "[panel]\nheight = 40\nedge = 'bottom'\n");
    write(&dirs.user(), "[panel]\nheight = 50\n");
    let resolved = load(&dirs.sources(), Section::<Panel>::new("panel").unwrap());
    assert_eq!(
        resolved.settings,
        Panel {
            height: 50,
            edge: "bottom".into()
        }
    );
    assert_eq!(resolved.diagnostics, Vec::<Diagnostic>::new());
}

#[test]
fn reports_and_skips_a_file_that_cannot_be_read() {
    let dirs = Dirs::new();
    write(&dirs.system(), "[panel]\nheight = 40\n");
    // A directory where the user's file should be.
    fs::create_dir_all(dirs.user()).unwrap();
    let resolved = load(&dirs.sources(), Section::<Panel>::new("panel").unwrap());
    assert_eq!(resolved.settings, panel(40));
    assert_eq!(resolved.diagnostics.len(), 1);
    assert_eq!(resolved.diagnostics[0].file, dirs.user());
    assert!(matches!(
        resolved.diagnostics[0].problem,
        Problem::Unreadable { .. }
    ));
}
