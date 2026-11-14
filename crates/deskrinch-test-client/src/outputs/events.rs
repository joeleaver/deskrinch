//! What `wlr-output-management`'s events build up: the heads, their
//! modes, and the outcome of a configuration.

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
use wayland_protocols_wlr::output_management::v1::client::zwlr_output_mode_v1::{
    self, ZwlrOutputModeV1,
};

use deskrinch_test_wayland::Transform;

use crate::layout::{Mode, OutputState};

/// One head (output) as the manager describes it.
#[derive(Debug)]
pub(super) struct Head {
    pub(super) proxy: ZwlrOutputHeadV1,
    pub(super) state: OutputState,
    /// Its modes, as the compositor lists them.
    pub(super) modes: Vec<ZwlrOutputModeV1>,
    /// Its current mode, described in [`State::modes`].
    pub(super) current: Option<ZwlrOutputModeV1>,
}

/// What the manager's events build up.
#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) heads: Vec<Head>,
    /// Every mode the compositor described, and what it is.
    pub(super) modes: Vec<(ZwlrOutputModeV1, Mode)>,
    /// The serial of the last complete description.
    pub(super) serial: Option<u32>,
    /// `Ok` once applied, `Err` with the reason once not.
    pub(super) outcome: Option<Result<(), String>>,
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
                    mode: None,
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
