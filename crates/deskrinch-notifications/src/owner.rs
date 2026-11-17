//! Who owns a name on the bus, said for a person: the connection's unique
//! name and its program, so that "the name is taken" names who took it.

use std::fs;

use deskrinch_bus::Bus;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// The owner of `name` on `bus`, as one phrase: `":1.42 (plasmashell, pid
/// 1234)"`, less what the bus or `/proc` does not tell. `None` when the
/// bus cannot be asked or no one owns the name (any more).
pub(crate) fn describe(bus: &Bus, name: &'static str) -> Option<String> {
    zbus::block_on(async {
        let connection = bus.builder().build().await.ok()?;
        let proxy = DBusProxy::new(&connection).await.ok()?;
        let name = BusName::try_from(name).ok()?;
        let unique = proxy.get_name_owner(name.clone()).await.ok()?;
        let pid = proxy.get_connection_unix_process_id(name).await.ok();
        Some(phrase(unique.as_str(), pid, pid.and_then(program)))
    })
}

/// The phrase for the connection `unique`, with its process `pid` and that
/// process's `program`, as far as they are known.
fn phrase(unique: &str, pid: Option<u32>, program: Option<String>) -> String {
    match (pid, program) {
        (Some(pid), Some(program)) => format!("{unique} ({program}, pid {pid})"),
        (Some(pid), None) => format!("{unique} (pid {pid})"),
        (None, _) => unique.to_owned(),
    }
}

/// The program name of process `pid` (`/proc/<pid>/comm`), one line.
fn program(pid: u32) -> Option<String> {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let comm = comm.trim_end();
    (!comm.is_empty()).then(|| comm.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_is_known() {
        assert_eq!(
            phrase(":1.42", Some(1234), Some("plasmashell".into())),
            ":1.42 (plasmashell, pid 1234)"
        );
        assert_eq!(phrase(":1.42", Some(1234), None), ":1.42 (pid 1234)");
        assert_eq!(phrase(":1.42", None, None), ":1.42");
    }

    #[test]
    fn reads_its_own_program() {
        let own = program(std::process::id()).unwrap();
        assert!(!own.is_empty() && !own.contains('\n'));
    }
}
