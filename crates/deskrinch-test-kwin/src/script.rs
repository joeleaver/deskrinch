//! The KWin script that reports every window, and its loading, running and
//! unloading through KWin's scripting interface.
//!
//! The script calls, through `callDBus`, `Window` once a window with its
//! record (a JSON object of KWin's property names) and then `Done` with how
//! many it reported, on [`INTERFACE`] at `/` of the name it is given. Every
//! argument is a string: `callDBus` passes a script's numbers as doubles,
//! which a method taking an integer would refuse.

use std::io::Write as _;

use zbus::Connection;

use crate::Error;
use crate::Kwin;
use crate::names::{KWIN, SCRIPT, SCRIPTING};
use crate::window::{FLAGS, PROPERTIES, SKIPS};

/// The interface the script reports to.
pub(crate) const INTERFACE: &str = "org.deskrinch.TestKwin";

/// The script, reporting to the bus name `service`: each window's record
/// holds the properties [`Window`](crate::Window) reads, by KWin's names.
#[must_use]
pub(crate) fn source(service: &str) -> String {
    let properties = PROPERTIES
        .into_iter()
        .chain(FLAGS.map(|(name, _)| name))
        .chain(SKIPS.map(|(name, _)| name))
        .fold(String::new(), |mut lines, name| {
            for part in ["        ", name, ": w.", name, ",\n"] {
                lines.push_str(part);
            }
            lines
        });
    format!(
        r#"// deskrinch-test-kwin: every window, as KWin holds it.
const service = "{service}";
function report(method, text) {{
    callDBus(service, "/", "{INTERFACE}", method, text);
}}
const windows = workspace.windowList();
for (let i = 0; i < windows.length; ++i) {{
    const w = windows[i];
    const g = w.frameGeometry;
    report("Window", JSON.stringify({{
{properties}        output: w.output ? w.output.name : null,
        frameGeometry: [g.x, g.y, g.width, g.height]
    }}));
}}
report("Done", String(windows.length));
"#
    )
}

/// A script loaded into KWin and run; unloaded when dropped.
///
/// A test that dies before dropping it leaves the script loaded until KWin
/// ends, which is when its session ends (the end of `cargo xtask
/// compositor-tests`' run, or of `cargo xtask session`'s command). It has
/// already run once and registered nothing, so it does nothing more; its
/// plugin name, unique to the process and the call, keeps it from clashing
/// with a later load in the same session.
#[derive(Debug)]
pub(crate) struct Loaded {
    connection: Connection,
    plugin: String,
    /// The script's file, which KWin reads when it loads it.
    _file: tempfile::NamedTempFile,
}

impl Loaded {
    /// Writes `source` to a file, loads it into KWin under `plugin`, and
    /// runs it, on `_kwin`'s bus, which `connection` is to.
    ///
    /// # Errors
    ///
    /// [`Error::File`] when the file cannot be written; [`Error::Bus`] when
    /// KWin cannot be asked; [`Error::NotLoaded`] when it will not load it.
    pub(crate) async fn run(
        _kwin: &Kwin,
        connection: &Connection,
        plugin: &str,
        source: &str,
    ) -> Result<Self, Error> {
        let mut file = tempfile::Builder::new()
            .prefix("deskrinch-test-kwin-")
            .suffix(".js")
            .tempfile()?;
        file.write_all(source.as_bytes())?;
        file.flush()?;
        let path = file.path().to_string_lossy().into_owned();
        let reply = connection
            .call_method(
                Some(KWIN),
                SCRIPTING.0,
                Some(SCRIPTING.1),
                "loadScript",
                &(path.as_str(), plugin),
            )
            .await?;
        let id: i32 = reply.body().deserialize()?;
        let loaded = Self {
            connection: connection.clone(),
            plugin: plugin.to_owned(),
            _file: file,
        };
        if id < 0 {
            return Err(Error::NotLoaded {
                plugin: plugin.to_owned(),
                id,
            });
        }
        connection
            .call_method(
                Some(KWIN),
                format!("/Scripting/Script{id}").as_str(),
                Some(SCRIPT),
                "run",
                &(),
            )
            .await?;
        Ok(loaded)
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        let unloaded = zbus::block_on(self.connection.call_method(
            Some(KWIN),
            SCRIPTING.0,
            Some(SCRIPTING.1),
            "unloadScript",
            &(self.plugin.as_str(),),
        ));
        // Nothing to do if KWin is gone: the script went with it.
        drop(unloaded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_reports_to_the_name_it_is_given() {
        let source = source("org.deskrinch.TestKwin.p42.n0");
        assert!(source.contains(r#"const service = "org.deskrinch.TestKwin.p42.n0";"#));
        assert!(
            source.contains(r#"callDBus(service, "/", "org.deskrinch.TestKwin", method, text);"#)
        );
        assert!(source.contains(r#"report("Done", String(windows.length));"#));
    }
}
