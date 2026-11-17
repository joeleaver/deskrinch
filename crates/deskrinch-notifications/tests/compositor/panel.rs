//! The real panel along the top of the output, for the room tests: what it
//! reserves the compositor takes from the column's height.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use deskrinch_settings::panel::Height;
use deskrinch_test_component::{Component, Renderer, programs};

/// The least height the panel takes, in logical pixels: its settings'.
pub(crate) const PANEL_LEAST: u32 = *Height::RANGE.start();

/// The panel, running along the top `height` logical pixels high; ended
/// when dropped, and its settings with it.
pub(crate) struct Panel {
    component: Option<Component>,
}

impl Panel {
    /// Starts the panel along the top, `height` high, and returns once it
    /// has presented.
    pub(crate) fn along_the_top(height: u32) -> Self {
        write_settings(height);
        let program = programs::program(deskrinch_shell::PANEL.package).unwrap();
        let renderer = Renderer::of_run();
        let component = Component::start(Command::new(program), renderer).unwrap();
        if let Err(why) = component.wait_presented_with(renderer) {
            panic!("the panel: {why}");
        }
        Self {
            component: Some(component),
        }
    }

    /// The running panel made `height` high, through its settings, which it
    /// follows: one change of what it reserves, with no new surface. The
    /// test waits for what the change does to the column.
    pub(crate) fn set_height(&self, height: u32) {
        assert!(self.component.is_some(), "the panel has ended");
        write_settings(height);
    }
}

/// The panel's settings: along the top, `height` high.
fn write_settings(height: u32) {
    let path = settings_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("[panel]\nedge = \"top\"\nheight = {height}\n"),
    )
    .unwrap();
}

impl Drop for Panel {
    fn drop(&mut self) {
        // Killed and reaped: its surface, and what it reserves, go with it.
        drop(self.component.take());
        drop(fs::remove_file(settings_file()));
    }
}

/// The session's configuration file, in its own `XDG_CONFIG_HOME`.
fn settings_file() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME").expect("the session's XDG_CONFIG_HOME");
    PathBuf::from(config).join("deskrinch/config.toml")
}
