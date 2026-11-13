//! A nested session's outputs through `wlr-output-management-unstable-v1`
//! (sway, labwc and other wlroots compositors): read, and set as a whole.
//!
//! Every configuration names every head, and states everything of each one
//! it turns on (mode, position, scale, transform): the protocol leaves a
//! property not set to the compositor, and sway puts an output turned on
//! without a position at (0,0).
//!
//! KWin is not covered here: its own protocol, `kde_output_management_v2`,
//! needs `kde_output_device_v2`, and KWin 6.6 sends that interface's modes
//! an event (`flags`, since version 19) to clients bound at older versions,
//! which the Rust bindings (`wayland-protocols-plasma` 0.3.12) cannot read.
//! Under KWin the outputs are read and set with `kscreen-doctor` instead
//! ([`crate::kscreen`]).

use std::os::unix::net::UnixStream;
use std::path::Path;

use wayland_client::protocol::wl_output::Transform as WlTransform;
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop, event_created_child,
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
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_mode_v1::{
    self, ZwlrOutputModeV1,
};

use deskrinch_test_wayland::{Deadline, Globals, Transform, dispatch_until};

use crate::layout::{Mode, OutputState};
use crate::{Error, PATIENCE, waited};

/// One head (output) as the manager describes it.
#[derive(Debug)]
struct Head {
    proxy: ZwlrOutputHeadV1,
    state: OutputState,
    /// Its modes, as the compositor lists them.
    modes: Vec<ZwlrOutputModeV1>,
    /// Its current mode, described in [`State::modes`].
    current: Option<ZwlrOutputModeV1>,
}

/// What the manager's events build up.
#[derive(Debug, Default)]
struct State {
    heads: Vec<Head>,
    /// Every mode the compositor described, and what it is.
    modes: Vec<(ZwlrOutputModeV1, Mode)>,
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

/// A connection to the manager, its heads described.
struct Manager {
    queue: EventQueue<State>,
    state: State,
    proxy: ZwlrOutputManagerV1,
    deadline: Deadline,
}

impl Manager {
    fn bind(connection: &Connection) -> Result<Self, Error> {
        let deadline = Deadline::after(PATIENCE);
        let mut queue = connection.new_event_queue();
        let mut state = State::default();
        let globals = Globals::list(connection, &mut queue, &mut state, deadline)
            .map_err(|error| waited(error, "listing the globals"))?;
        let proxy: ZwlrOutputManagerV1 = globals
            .bind(&queue.handle(), 1..=4, ())
            .map_err(|_| Error::NoOutputManagement)?;
        dispatch_until(&mut queue, &mut state, deadline, |state| {
            state.serial.is_some()
        })
        .map_err(|error| waited(error, "learning the outputs"))?;
        Ok(Self {
            queue,
            state,
            proxy,
            deadline,
        })
    }

    fn layout(&self) -> Vec<OutputState> {
        self.state
            .heads
            .iter()
            .map(|head| {
                let mode = head.current.as_ref().and_then(|current| {
                    self.state
                        .modes
                        .iter()
                        .find(|(mode, _)| mode == current)
                        .map(|(_, described)| *described)
                });
                OutputState {
                    mode: mode.unwrap_or(head.state.mode),
                    ..head.state.clone()
                }
            })
            .collect()
    }

    /// Makes the outputs `layout`: every head named, each one on stated in
    /// full. A head `layout` does not name is stated as it is.
    fn apply(mut self, layout: &[OutputState]) -> Result<(), Error> {
        let qh = self.queue.handle();
        for output in layout {
            if !self.state.heads.iter().any(|h| h.state.name == output.name) {
                return Err(Error::UnknownOutput(output.name.clone()));
            }
        }
        let refused = |reason: String| Error::OutputRefused {
            name: "the outputs".to_owned(),
            reason,
        };
        let serial = self.state.serial.unwrap_or_default();
        let configuration = self.proxy.create_configuration(serial, &qh, ());
        for head in &self.state.heads {
            let target = layout
                .iter()
                .find(|output| output.name == head.state.name)
                .unwrap_or(&head.state);
            if !target.enabled {
                configuration.disable_head(&head.proxy);
                continue;
            }
            let settings = configuration.enable_head(&head.proxy, &qh, ());
            let listed = head.modes.iter().find(|proxy| {
                self.state
                    .modes
                    .iter()
                    .any(|(mode, described)| mode == *proxy && *described == target.mode)
            });
            match listed {
                Some(mode) => settings.set_mode(mode),
                None => settings.set_custom_mode(
                    signed(target.mode.width),
                    signed(target.mode.height),
                    signed(target.mode.refresh_mhz),
                ),
            }
            settings.set_position(target.position.0, target.position.1);
            settings.set_scale(target.scale);
            let transform = WlTransform::try_from(target.transform.to_wayland())
                .map_err(|()| refused(format!("transform {:?}", target.transform)))?;
            settings.set_transform(transform);
        }
        configuration.apply();
        let applied = dispatch_until(&mut self.queue, &mut self.state, self.deadline, |state| {
            state.outcome.is_some()
        })
        .map_err(|error| waited(error, "changing the outputs"));
        configuration.destroy();
        self.proxy.stop();
        applied?;
        self.state
            .outcome
            .take()
            .unwrap_or_else(|| Err("no answer".into()))
            .map_err(refused)
    }
}

/// `value` as the protocol's `int`, saturating (no output is that large).
fn signed(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// The outputs of the compositor `WAYLAND_DISPLAY` names.
pub(crate) fn layout_of_env() -> Result<Vec<OutputState>, Error> {
    let connection = Connection::connect_to_env().map_err(Error::Connect)?;
    Ok(Manager::bind(&connection)?.layout())
}

/// Makes the outputs of the compositor `WAYLAND_DISPLAY` names `layout`,
/// and returns once it has applied it.
pub(crate) fn apply_to_env(layout: &[OutputState]) -> Result<(), Error> {
    let connection = Connection::connect_to_env().map_err(Error::Connect)?;
    Manager::bind(&connection)?.apply(layout)
}

/// A connection to the compositor listening on the Wayland socket at
/// `socket`.
fn connect_at(socket: &Path) -> Result<Connection, Error> {
    let stream = UnixStream::connect(socket).map_err(|source| Error::Socket {
        path: socket.to_owned(),
        source,
    })?;
    Connection::from_socket(stream).map_err(Error::Connect)
}

/// The outputs of the wlroots compositor listening on the Wayland socket at
/// `socket`, as it reports them: for a session's runner, which is not one
/// of its clients.
///
/// # Errors
///
/// When the compositor does not offer `zwlr_output_manager_v1`
/// ([`Error::NoOutputManagement`]), or the connection fails.
pub fn wlroots_outputs_at(socket: &Path) -> Result<Vec<OutputState>, Error> {
    Ok(Manager::bind(&connect_at(socket)?)?.layout())
}

/// Changes the output named `name` of the compositor `WAYLAND_DISPLAY`
/// names, every other output stated as it is, and returns once the
/// compositor has applied the change, with the output's state from before
/// it. Clients learn of the change a moment later, through their own
/// `wl_output` events. A test changes an output with
/// [`change_output`](crate::change_output), which puts it back.
///
/// # Errors
///
/// When the compositor does not offer `zwlr_output_manager_v1`
/// ([`Error::NoOutputManagement`]), has no output so named, refuses the
/// change, or the connection fails.
pub fn configure_output(name: &str, change: OutputChange) -> Result<OutputState, Error> {
    let connection = Connection::connect_to_env().map_err(Error::Connect)?;
    let manager = Manager::bind(&connection)?;
    let before = manager
        .layout()
        .into_iter()
        .find(|output| output.name == name && output.enabled)
        .ok_or_else(|| Error::UnknownOutput(name.to_owned()))?;
    manager.apply(&[before.changed(change)])?;
    Ok(before)
}

/// Applies `change` to every output that is on, of the compositor
/// listening on the Wayland socket at `socket`, each stated in full, and
/// returns once it has applied it: how a nested session sets its outputs up
/// before its first client connects.
///
/// # Errors
///
/// As [`configure_output`].
pub fn configure_every_output(socket: &Path, change: OutputChange) -> Result<(), Error> {
    let manager = Manager::bind(&connect_at(socket)?)?;
    let target: Vec<OutputState> = manager
        .layout()
        .iter()
        .map(|output| output.changed(change))
        .collect();
    manager.apply(&target)
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
                state: OutputState {
                    name: String::new(),
                    enabled: false,
                    position: (0, 0),
                    mode: Mode {
                        width: 0,
                        height: 0,
                        refresh_mhz: 0,
                    },
                    scale: 1.0,
                    transform: Transform::Normal,
                },
                modes: Vec::new(),
                current: None,
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
        let output = &mut head.state;
        match event {
            zwlr_output_head_v1::Event::Name { name } => output.name = name,
            zwlr_output_head_v1::Event::Enabled { enabled } => output.enabled = enabled != 0,
            zwlr_output_head_v1::Event::Position { x, y } => output.position = (x, y),
            zwlr_output_head_v1::Event::CurrentMode { mode } => head.current = Some(mode),
            zwlr_output_head_v1::Event::Scale { scale } => output.scale = scale,
            zwlr_output_head_v1::Event::Transform { transform } => {
                let value = match transform {
                    WEnum::Value(transform) => transform.into(),
                    WEnum::Unknown(value) => value,
                };
                output.transform = Transform::from_wayland(value).unwrap_or_default();
            }
            zwlr_output_head_v1::Event::Mode { mode } => {
                head.modes.push(mode.clone());
                state.modes.push((
                    mode,
                    Mode {
                        width: 0,
                        height: 0,
                        refresh_mhz: 0,
                    },
                ));
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

impl Dispatch<ZwlrOutputModeV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, mode)) = state.modes.iter_mut().find(|(mode, _)| mode == proxy) else {
            return;
        };
        match event {
            zwlr_output_mode_v1::Event::Size { width, height } => {
                mode.width = width.try_into().unwrap_or_default();
                mode.height = height.try_into().unwrap_or_default();
            }
            zwlr_output_mode_v1::Event::Refresh { refresh } => {
                mode.refresh_mhz = refresh.try_into().unwrap_or_default();
            }
            _ => {}
        }
    }
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

delegate_noop!(State: ZwlrOutputConfigurationHeadV1);
