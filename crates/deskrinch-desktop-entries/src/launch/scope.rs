//! Placing a launched process in a transient systemd scope of its own.
//!
//! Following systemd's desktop-environment conventions
//! (<https://systemd.io/DESKTOP_ENVIRONMENTS/>), the scope is called
//! `app-<launcher>-<ApplicationID>-<RANDOM>.scope` (`app-<ApplicationID>-…`
//! without a launcher) and placed in `app.slice`. The application ID is the
//! desktop-file ID without `.desktop`, escaped as systemd escapes unit-name
//! parts (`-` becomes `\x2d`, so the dashes stay separators). The scope is
//! collected when it ends, even after a failure, as GNOME's is.
//!
//! The scope is created with `StartTransientUnit` naming the process's PID,
//! and the launch waits for systemd's `JobRemoved` signal for that job
//! before the process may exec.

use std::collections::hash_map::RandomState;
use std::future::poll_fn;
use std::hash::{BuildHasher, Hasher};
use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use zbus::message::Type as MessageType;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath, Variant};

use super::bus::with_timeout;
use crate::error::BoxedError;

/// systemd's bus name, object and manager interface.
pub(super) const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";

/// How long systemd has to run the job that starts the scope.
const JOB_TIMEOUT: Duration = Duration::from_secs(5);

/// The unit name for a new scope of `app_id`.
pub(super) fn unit_name(launcher: Option<&str>, app_id: &str, random: u64) -> String {
    let app_id = escape(app_id);
    match launcher {
        Some(launcher) => format!("app-{}-{app_id}-{random:016x}.scope", escape(launcher)),
        None => format!("app-{app_id}-{random:016x}.scope"),
    }
}

/// A random number for a unit name, from the standard library's per-process
/// random hasher keys mixed with `pid`.
pub(super) fn random_part(pid: u32) -> u64 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(pid);
    hasher.finish()
}

/// Escapes a unit-name part as `systemd-escape` does: ASCII letters, digits,
/// `:`, `_` and `.` (but not a leading `.`) are kept, every other byte
/// becomes `\xNN`.
fn escape(part: &str) -> String {
    part.bytes()
        .enumerate()
        .map(|(index, byte)| {
            let keep = byte.is_ascii_alphanumeric()
                || byte == b':'
                || byte == b'_'
                || (byte == b'.' && index > 0);
            if keep {
                char::from(byte).to_string()
            } else {
                format!("\\x{byte:02x}")
            }
        })
        .collect()
}

/// Starts the scope `unit` around `pid` and waits until systemd has run the
/// job that starts it.
pub(super) async fn start_scope(
    connection: &Connection,
    unit: &str,
    description: &str,
    pid: u32,
) -> Result<(), BoxedError> {
    // Subscribed before the call, so the signal cannot be missed. Matched
    // by path, interface and member: a rule naming the well-known sender
    // would not match signals, which carry systemd's unique name.
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .path(SYSTEMD_PATH)
        .interface(MANAGER)
        .member("JobRemoved")
        .build()?;
    let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
    let properties: Vec<(&str, Variant<'_>)> = vec![
        ("Description", Variant::from(description)),
        ("PIDs", Variant::from(vec![pid])),
        ("Slice", Variant::from("app.slice")),
        ("CollectMode", Variant::from("inactive-or-failed")),
    ];
    let auxiliary: Vec<(&str, Vec<(&str, Variant<'_>)>)> = Vec::new();
    let reply = connection
        .call_method(
            Some(SYSTEMD),
            SYSTEMD_PATH,
            Some(MANAGER),
            "StartTransientUnit",
            &(unit, "fail", properties, auxiliary),
        )
        .await?;
    let job: OwnedObjectPath = reply.body().deserialize()?;
    let wait = async {
        loop {
            let message = poll_fn(|cx| Pin::new(&mut signals).poll_next(cx))
                .await
                .ok_or("the bus connection closed")??;
            let (_id, removed, _unit, result): (u32, OwnedObjectPath, String, String) =
                message.body().deserialize()?;
            if removed == job {
                return if result == "done" {
                    Ok(())
                } else {
                    Err(format!("systemd's job to start {unit} ended with {result:?}").into())
                };
            }
        }
    };
    with_timeout(wait, JOB_TIMEOUT)
        .await
        .unwrap_or_else(|| Err(format!("systemd did not start {unit} in time").into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_scopes_by_the_convention() {
        assert_eq!(
            unit_name(Some("deskrinch"), "org.kde.konsole", 0xab),
            "app-deskrinch-org.kde.konsole-00000000000000ab.scope"
        );
        assert_eq!(
            unit_name(None, "firefox", 1),
            "app-firefox-0000000000000001.scope"
        );
    }

    #[test]
    fn escapes_dashes_and_other_bytes() {
        assert_eq!(escape("my-app"), r"my\x2dapp");
        assert_eq!(escape("a b/ü"), r"a\x20b\x2f\xc3\xbc");
        assert_eq!(escape(".hidden.x"), r"\x2ehidden.x");
        assert_eq!(escape("a:b_c.d"), "a:b_c.d");
    }

    #[test]
    fn random_parts_differ() {
        assert_ne!(random_part(1), random_part(1));
    }
}
