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
use std::time::{Duration, Instant};

use wayland_client::protocol::wl_output::Transform as WlTransform;
use wayland_client::{Connection, EventQueue};
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_manager_v1::ZwlrOutputManagerV1;

use deskrinch_test_wayland::{Deadline, Globals, Transform, dispatch_until};

mod events;

use self::events::{Outcome, State};
use crate::layout::{Mode, OutputState, mode_to_set};
use crate::{Error, PATIENCE, waited};

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
                    mode,
                    ..head.state.clone()
                }
            })
            .collect()
    }

    /// Makes the outputs `layout`: every head named, each one on stated in
    /// full. A head `layout` does not name is stated as it is. Nothing is
    /// sent unless every head can be stated ([`plan`]). Whether it was
    /// applied, or cancelled because the outputs changed meanwhile.
    fn apply(mut self, layout: &[OutputState]) -> Result<Applied, Error> {
        let qh = self.queue.handle();
        let plans = plan(&self.layout(), layout)?;
        let serial = self.state.serial.unwrap_or_default();
        let configuration = self.proxy.create_configuration(serial, &qh, ());
        for (head, planned) in self.state.heads.iter().zip(plans) {
            let Planned::On {
                mode,
                position,
                scale,
                transform,
            } = planned
            else {
                configuration.disable_head(&head.proxy);
                continue;
            };
            let settings = configuration.enable_head(&head.proxy, &qh, ());
            let listed = head.modes.iter().find(|proxy| {
                self.state
                    .modes
                    .iter()
                    .any(|(listed, described)| listed == *proxy && *described == mode)
            });
            match listed {
                Some(listed) => settings.set_mode(listed),
                None => settings.set_custom_mode(
                    signed(mode.width),
                    signed(mode.height),
                    signed(mode.refresh_mhz),
                ),
            }
            settings.set_position(position.0, position.1);
            settings.set_scale(scale);
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
        match self.state.outcome.take() {
            Some(Outcome::Succeeded) => Ok(Applied::Done),
            Some(Outcome::Cancelled) => Ok(Applied::Cancelled),
            Some(Outcome::Failed) | None => Err(Error::OutputRefused {
                name: "the outputs".to_owned(),
                reason: "the compositor could not apply the configuration".to_owned(),
            }),
        }
    }
}

/// What came of one configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Applied {
    /// Applied.
    Done,
    /// Cancelled: the outputs changed since it was made.
    Cancelled,
}

/// Makes the outputs on `connection` what `target` makes of them as they
/// now are, reading them and making the configuration again each time one
/// is cancelled ([`retry_cancelled`]): one home for every apply through
/// `wlr-output-management`.
fn applying(
    connection: &Connection,
    target: impl Fn(&[OutputState]) -> Vec<OutputState>,
) -> Result<(), Error> {
    retry_cancelled(
        || {
            let manager = Manager::bind(connection)?;
            let layout = target(&manager.layout());
            manager.apply(&layout)
        },
        PATIENCE,
    )
}

/// Runs `attempt` again each time its configuration is cancelled, which
/// the protocol means as "the outputs changed since; read again and try
/// again", for at most `patience`. A configuration that failed is not
/// retried: that is the compositor refusing it.
fn retry_cancelled(
    mut attempt: impl FnMut() -> Result<Applied, Error>,
    patience: Duration,
) -> Result<(), Error> {
    let deadline = Instant::now() + patience;
    let mut cancelled = 0;
    loop {
        match attempt()? {
            Applied::Done => return Ok(()),
            Applied::Cancelled => cancelled += 1,
        }
        if Instant::now() >= deadline {
            return Err(Error::Cancelled {
                attempts: cancelled,
                patience,
            });
        }
    }
}

/// What a configuration says of one head.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Planned {
    /// `disable_head`.
    Off,
    /// `enable_head`, with everything stated.
    On {
        mode: Mode,
        position: (i32, i32),
        scale: f64,
        transform: WlTransform,
    },
}

/// What a configuration that makes the outputs `layout` says of each head
/// of `current`, in its order: a head `layout` does not name is stated as
/// it is. Refused, before anything is sent, for an output `layout` names
/// that is not there, and for one to be on with no known mode.
fn plan(current: &[OutputState], layout: &[OutputState]) -> Result<Vec<Planned>, Error> {
    if let Some(unknown) = layout
        .iter()
        .find(|output| !current.iter().any(|head| head.name == output.name))
    {
        return Err(Error::UnknownOutput(unknown.name.clone()));
    }
    current
        .iter()
        .map(|head| {
            let target = layout
                .iter()
                .find(|output| output.name == head.name)
                .unwrap_or(head);
            if !target.enabled {
                return Ok(Planned::Off);
            }
            let transform = WlTransform::try_from(target.transform.to_wayland()).map_err(|()| {
                Error::OutputRefused {
                    name: target.name.clone(),
                    reason: format!("transform {:?}", target.transform),
                }
            })?;
            Ok(Planned::On {
                mode: mode_to_set(target)?,
                position: target.position,
                scale: target.scale,
                transform,
            })
        })
        .collect()
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
    applying(&connection, |_| layout.to_vec())
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
    let target = before.changed(change);
    applying(&connection, |_| vec![target.clone()])?;
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
    applying(&connect_at(socket)?, |current| {
        current
            .iter()
            .map(|output| output.changed(change))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(name: &str, mode: Option<Mode>) -> OutputState {
        OutputState {
            name: name.to_owned(),
            enabled: true,
            position: (0, 0),
            mode,
            scale: 1.0,
            transform: Transform::Normal,
        }
    }

    const MODE: Mode = Mode {
        width: 1280,
        height: 720,
        refresh_mhz: 0,
    };

    #[test]
    fn a_cancelled_configuration_is_made_again() {
        let attempts = std::cell::Cell::new(0);
        retry_cancelled(
            || {
                attempts.set(attempts.get() + 1);
                Ok(if attempts.get() == 1 {
                    Applied::Cancelled
                } else {
                    Applied::Done
                })
            },
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn cancelled_until_patience_runs_out_says_how_often() {
        let attempts = std::cell::Cell::new(0_u32);
        let error = retry_cancelled(
            || {
                attempts.set(attempts.get() + 1);
                std::thread::sleep(Duration::from_millis(10));
                Ok(Applied::Cancelled)
            },
            Duration::from_millis(100),
        )
        .unwrap_err();
        let said = error.to_string();
        let Error::Cancelled {
            attempts: counted, ..
        } = error
        else {
            panic!("{error:?}");
        };
        assert!(counted >= 2, "{counted}");
        assert_eq!(counted, attempts.get());
        assert!(
            said.contains(&format!("each of {counted} configurations")),
            "{said}"
        );
    }

    #[test]
    fn a_refusal_is_not_retried() {
        let attempts = std::cell::Cell::new(0);
        let error = retry_cancelled(
            || {
                attempts.set(attempts.get() + 1);
                Err(Error::OutputRefused {
                    name: "the outputs".into(),
                    reason: "failed".into(),
                })
            },
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(error, Error::OutputRefused { .. }));
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn every_head_is_stated_and_one_not_named_as_it_is() {
        let current = [output("A", Some(MODE)), output("B", Some(MODE))];
        let off = OutputState {
            enabled: false,
            ..current[0].clone()
        };
        let plans = plan(&current, &[off]).unwrap();
        assert_eq!(plans[0], Planned::Off);
        assert_eq!(
            plans[1],
            Planned::On {
                mode: MODE,
                position: (0, 0),
                scale: 1.0,
                transform: WlTransform::Normal,
            }
        );
    }

    #[test]
    fn an_output_to_be_on_without_a_mode_is_refused_before_anything_is_sent() {
        // As a head that is off may come: no current mode.
        let current = [output("A", Some(MODE)), output("B", None)];
        assert!(matches!(
            plan(&current, &[]),
            Err(Error::NoMode(name)) if name == "B"
        ));
        // Off, it needs none.
        let off = OutputState {
            enabled: false,
            ..current[1].clone()
        };
        assert!(plan(&current, &[off]).is_ok());
        assert!(matches!(
            plan(&current, &[output("C", Some(MODE))]),
            Err(Error::UnknownOutput(name)) if name == "C"
        ));
    }
}
