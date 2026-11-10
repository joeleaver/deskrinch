//! D-Bus activation of a `DBusActivatable` application through
//! `org.freedesktop.Application`.
//!
//! The application's bus name is its desktop-file ID without `.desktop`,
//! and its object path is that name with `.` replaced by `/` and `-` by `_`.
//! The bus starts the application from its D-Bus service file if it is not
//! running. `Activate` opens it, `Open` opens URIs in it, and
//! `ActivateAction` runs one of its actions. The activation token travels
//! in the platform data, as `activation-token` (Wayland) and
//! `desktop-startup-id` (X11), as GLib sends it.

use std::collections::HashMap;

use zbus::names::WellKnownName;
use zbus::{Connection, ObjectPath, Variant};

const INTERFACE: &str = "org.freedesktop.Application";

/// What to ask the application to do.
pub(super) enum Request<'a> {
    /// `Activate`.
    Activate,
    /// `Open` these URIs.
    Open(Vec<String>),
    /// `ActivateAction` with this action.
    Action(&'a str),
}

/// The object path of the application with this bus name.
pub(super) fn object_path(bus_name: &str) -> String {
    format!("/{}", bus_name.replace('.', "/").replace('-', "_"))
}

/// Whether `bus_name` is a valid well-known bus name.
pub(super) fn valid_bus_name(bus_name: &str) -> bool {
    WellKnownName::try_from(bus_name).is_ok() && ObjectPath::try_from(object_path(bus_name)).is_ok()
}

/// Sends `request` to the application `bus_name`.
pub(super) async fn activate(
    connection: &Connection,
    bus_name: &str,
    request: Request<'_>,
    activation_token: Option<&str>,
) -> zbus::Result<()> {
    let mut platform_data: HashMap<&str, Variant<'_>> = HashMap::new();
    if let Some(token) = activation_token {
        platform_data.insert("activation-token", Variant::from(token));
        platform_data.insert("desktop-startup-id", Variant::from(token));
    }
    let path = object_path(bus_name);
    let destination = Some(bus_name);
    let interface = Some(INTERFACE);
    match request {
        Request::Activate => {
            connection
                .call_method(
                    destination,
                    path.as_str(),
                    interface,
                    "Activate",
                    &(platform_data,),
                )
                .await?
        }
        Request::Open(uris) => {
            connection
                .call_method(
                    destination,
                    path.as_str(),
                    interface,
                    "Open",
                    &(uris, platform_data),
                )
                .await?
        }
        Request::Action(action) => {
            let parameter: Vec<Variant<'_>> = Vec::new();
            connection
                .call_method(
                    destination,
                    path.as_str(),
                    interface,
                    "ActivateAction",
                    &(action, parameter, platform_data),
                )
                .await?
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_object_path() {
        assert_eq!(object_path("org.example.My-App"), "/org/example/My_App");
    }

    #[test]
    fn validates_bus_names() {
        assert!(valid_bus_name("org.example.App"));
        assert!(!valid_bus_name("firefox"));
        assert!(!valid_bus_name("org..x"));
    }
}
