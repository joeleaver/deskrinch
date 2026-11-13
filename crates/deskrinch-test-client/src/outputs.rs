//! Changing a nested session's outputs from a test: their scale and
//! transform, and whether they are on, through
//! `wlr-output-management-unstable-v1` (sway, labwc and other wlroots
//! compositors).
//!
//! KWin is not covered: its own protocol, `kde_output_management_v2`, needs
//! `kde_output_device_v2`, and KWin 6.6 sends that interface's modes an event
//! (`flags`, since version 19) to clients bound at older versions, which the
//! Rust bindings (`wayland-protocols-plasma` 0.3.12) cannot read.

use std::os::unix::net::UnixStream;
use std::path::Path;

use wayland_client::protocol::wl_output::Transform as WlTransform;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop, event_created_child,
};
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1;
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_configuration_v1::{
    self, ZwlrOutputConfigurationV1,
};
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::{
    self, ZwlrOutputHeadV1,
};
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_manager_v1::{
    self, ZwlrOutputManagerV1,
};
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_mode_v1::ZwlrOutputModeV1;

use deskrinch_test_wayland::{Deadline, Globals, Transform, dispatch_until};

use crate::{Error, PATIENCE, waited};

/// One head (output) as the manager describes it.
#[derive(Debug)]
struct Head {
    proxy: ZwlrOutputHeadV1,
    name: Option<String>,
    enabled: bool,
    settings: OutputSettings,
}

/// What the manager's events build up.
#[derive(Debug, Default)]
struct State {
    heads: Vec<Head>,
    /// The serial of the last complete description.
    serial: Option<u32>,
    /// `Ok` once applied, `Err` with the reason once not.
    outcome: Option<Result<(), String>>,
}

/// What to change about an output; `None` leaves that as it is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OutputChange {
    /// The scale: how many physical pixels a logical one covers.
    pub scale: Option<f64>,
    /// The transform.
    pub transform: Option<Transform>,
}

/// An output's scale and transform, as the compositor reports them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSettings {
    /// The scale.
    pub scale: f64,
    /// The transform.
    pub transform: Transform,
}

impl From<OutputSettings> for OutputChange {
    fn from(settings: OutputSettings) -> Self {
        Self {
            scale: Some(settings.scale),
            transform: Some(settings.transform),
        }
    }
}

/// Changes the output named `name` of the compositor `WAYLAND_DISPLAY`
/// names, and returns once the compositor has applied the change, with the
/// output's settings from before it (to put back). Every other output keeps
/// its state. Clients learn of the change a moment later, through their own
/// `wl_output` events.
///
/// # Errors
///
/// When the compositor does not offer `zwlr_output_manager_v1`
/// ([`Error::NoOutputManagement`]), has no output so named, refuses the
/// change, or the connection fails.
pub fn configure_output(name: &str, change: OutputChange) -> Result<OutputSettings, Error> {
    let connection = Connection::connect_to_env().map_err(Error::Connect)?;
    let before = configure(&connection, Some(name), Apply::Change(change))?;
    before
        .into_iter()
        .next()
        .ok_or_else(|| Error::UnknownOutput(name.to_owned()))
}

/// Applies `change` to every output of the compositor listening on the
/// Wayland socket at `socket`, and returns once it has applied it: how a
/// nested session sets its outputs up before its first client connects.
///
/// # Errors
///
/// As [`configure_output`].
pub fn configure_every_output(socket: &Path, change: OutputChange) -> Result<(), Error> {
    let stream = UnixStream::connect(socket).map_err(|source| Error::Socket {
        path: socket.to_owned(),
        source,
    })?;
    let connection = Connection::from_socket(stream).map_err(Error::Connect)?;
    configure(&connection, None, Apply::Change(change)).map(drop)
}

/// Turns the output named `name` of the compositor `WAYLAND_DISPLAY` names
/// on or off, and returns once the compositor has done it.
///
/// # Errors
///
/// As [`configure_output`].
pub(crate) fn set_enabled(name: &str, enabled: bool) -> Result<(), Error> {
    let connection = Connection::connect_to_env().map_err(Error::Connect)?;
    let changed = configure(&connection, Some(name), Apply::Enable(enabled))?;
    if changed.is_empty() {
        return Err(Error::UnknownOutput(name.to_owned()));
    }
    Ok(())
}

/// What a configuration does to the heads it is for.
#[derive(Debug, Clone, Copy)]
enum Apply {
    /// Changes their settings; they stay on.
    Change(OutputChange),
    /// Turns them on or off.
    Enable(bool),
}

/// Applies `apply` to the head named `which`, or to every head for `None`,
/// and returns the settings the heads it was for had before. Only a head
/// that is on can be changed; turning one on finds it off too.
fn configure(
    connection: &Connection,
    which: Option<&str>,
    apply: Apply,
) -> Result<Vec<OutputSettings>, Error> {
    let deadline = Deadline::after(PATIENCE);
    let mut queue = connection.new_event_queue();
    let mut state = State::default();
    let globals = Globals::list(connection, &mut queue, &mut state, deadline)
        .map_err(|error| waited(error, "listing the globals"))?;
    let qh = queue.handle();
    let manager: ZwlrOutputManagerV1 = globals
        .bind(&qh, 1..=4, ())
        .map_err(|_| Error::NoOutputManagement)?;
    dispatch_until(&mut queue, &mut state, deadline, |state| {
        state.serial.is_some()
    })
    .map_err(|error| waited(error, "learning the outputs"))?;
    let described = which.unwrap_or("every output");
    let refused = |reason: String| Error::OutputRefused {
        name: described.to_owned(),
        reason,
    };
    let chosen = |head: &Head| {
        (head.enabled || matches!(apply, Apply::Enable(true)))
            && (which.is_none() || head.name.as_deref() == which)
    };
    if let Some(name) = which
        && !state.heads.iter().any(chosen)
    {
        return Err(Error::UnknownOutput(name.to_owned()));
    }
    let serial = state.serial.unwrap_or_default();
    let configuration = manager.create_configuration(serial, &qh, ());
    let mut before = Vec::new();
    // A configuration names every head.
    for head in &state.heads {
        let enabled = match apply {
            Apply::Enable(enabled) if chosen(head) => enabled,
            _ => head.enabled,
        };
        if !enabled {
            configuration.disable_head(&head.proxy);
            if chosen(head) {
                before.push(head.settings);
            }
            continue;
        }
        let settings = configuration.enable_head(&head.proxy, &qh, ());
        if chosen(head) {
            before.push(head.settings);
            if let Apply::Change(change) = apply {
                if let Some(scale) = change.scale {
                    settings.set_scale(scale);
                }
                if let Some(transform) = change.transform {
                    let transform = WlTransform::try_from(transform.to_wayland())
                        .map_err(|()| refused(format!("transform {transform:?}")))?;
                    settings.set_transform(transform);
                }
            }
        }
    }
    configuration.apply();
    let applied = dispatch_until(&mut queue, &mut state, deadline, |state| {
        state.outcome.is_some()
    })
    .map_err(|error| waited(error, "changing an output"));
    configuration.destroy();
    manager.stop();
    applied?;
    state
        .outcome
        .take()
        .unwrap_or_else(|| Err("no answer".into()))
        .map_err(refused)?;
    Ok(before)
}

impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => state.heads.push(Head {
                proxy: head,
                name: None,
                enabled: false,
                settings: OutputSettings {
                    scale: 1.0,
                    transform: Transform::Normal,
                },
            }),
            zwlr_output_manager_v1::Event::Done { serial } => state.serial = Some(serial),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(head) = state.heads.iter_mut().find(|head| head.proxy == *proxy) else {
            return;
        };
        match event {
            zwlr_output_head_v1::Event::Name { name } => head.name = Some(name),
            zwlr_output_head_v1::Event::Enabled { enabled } => head.enabled = enabled != 0,
            zwlr_output_head_v1::Event::Scale { scale } => head.settings.scale = scale,
            zwlr_output_head_v1::Event::Transform { transform } => {
                let value = match transform {
                    WEnum::Value(transform) => transform.into(),
                    WEnum::Unknown(value) => value,
                };
                head.settings.transform = Transform::from_wayland(value).unwrap_or_default();
            }
            zwlr_output_head_v1::Event::Finished => {
                state.heads.retain(|head| head.proxy != *proxy);
            }
            _ => {}
        }
    }

    event_created_child!(State, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.outcome = match event {
            zwlr_output_configuration_v1::Event::Succeeded => Some(Ok(())),
            zwlr_output_configuration_v1::Event::Failed => {
                Some(Err("the compositor could not apply it".into()))
            }
            zwlr_output_configuration_v1::Event::Cancelled => {
                Some(Err("the outputs changed meanwhile".into()))
            }
            _ => return,
        };
    }
}

delegate_noop!(State: ignore ZwlrOutputModeV1);
delegate_noop!(State: ZwlrOutputConfigurationHeadV1);
