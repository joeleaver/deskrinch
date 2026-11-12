//! The compositor's outputs: their names and their place in the global
//! layout, from `wl_output` and `zxdg_output_v1`.
//!
//! A name comes from `wl_output.name` (version 4) or, failing that, from
//! `zxdg_output_v1.name` (version 2). The layout (each output's logical
//! rectangle) comes from `zxdg_output_v1` and is applied atomically on
//! `wl_output.done`, or `zxdg_output_v1.done` where that is the one sent.
//!
//! [`Outputs`] lives in a crate's own dispatch state, which implements
//! [`OutputsHandler`] and hands the outputs' events to it:
//!
//! ```ignore
//! wayland_client::delegate_dispatch!(State: [WlOutput: OutputGlobal] => Outputs);
//! wayland_client::delegate_dispatch!(State: [ZxdgOutputV1: OutputGlobal] => Outputs);
//! wayland_client::delegate_dispatch!(State: [ZxdgOutputManagerV1: ()] => Outputs);
//! ```

use std::collections::{BTreeSet, HashMap};

use wayland_client::globals::Global;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::{
    self, ZxdgOutputManagerV1,
};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};

/// A rectangle in the compositor's global logical coordinate space (the
/// space `zxdg_output_v1` describes outputs in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Its left edge.
    pub x: i32,
    /// Its top edge.
    pub y: i32,
    /// Its width.
    pub width: i32,
    /// Its height.
    pub height: i32,
}

/// The user data of a bound output and its xdg-output description: the
/// output's registry name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputGlobal(pub u32);

/// A dispatch state that holds [`Outputs`].
pub trait OutputsHandler {
    /// The outputs.
    fn outputs(&mut self) -> &mut Outputs;

    /// An output's name or logical rectangle changed.
    fn outputs_changed(&mut self);
}

/// `wl_output.name` arrived in version 4.
const OUTPUT_NAME_SINCE: u32 = 4;
/// `wl_output.done` arrived in version 2.
const OUTPUT_DONE_SINCE: u32 = 2;
/// `wl_output.release` arrived in version 3.
const OUTPUT_RELEASE_SINCE: u32 = 3;
/// `zxdg_output_v1.name` arrived in version 2.
const XDG_NAME_SINCE: u32 = 2;
/// From version 3 `zxdg_output_v1.done` is no longer sent; `wl_output.done`
/// takes its place.
const XDG_DONE_REPLACED_SINCE: u32 = 3;

/// Whether the outputs `globals` offers will have names: every `wl_output`
/// is new enough to send one, or `zxdg_output_v1` can.
#[must_use]
pub fn names_available(globals: &[Global]) -> bool {
    let interface = |global: &&Global, name: &str| global.interface == name;
    let mut outputs = globals
        .iter()
        .filter(|global| interface(global, WlOutput::interface().name))
        .peekable();
    let wl_names =
        outputs.peek().is_some() && outputs.all(|global| global.version >= OUTPUT_NAME_SINCE);
    let xdg_names = globals.iter().any(|global| {
        interface(&global, ZxdgOutputManagerV1::interface().name)
            && global.version >= XDG_NAME_SINCE
    });
    wl_names || xdg_names
}

/// Whether `globals` describes the output layout (`zxdg_output_v1`).
#[must_use]
pub fn layout_available(globals: &[Global]) -> bool {
    globals
        .iter()
        .any(|global| global.interface == ZxdgOutputManagerV1::interface().name)
}

/// One bound output.
#[derive(Debug)]
struct Output {
    proxy: WlOutput,
    xdg: Option<ZxdgOutputV1>,
    name: Option<String>,
    /// The layout as sent since the last `done`.
    pending: Pending,
    /// The layout as of the last `done`.
    logical: Option<Rect>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Pending {
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
}

impl Output {
    /// Applies the pending layout; whether it changed.
    fn apply(&mut self) -> bool {
        let Pending {
            position: Some((x, y)),
            size: Some((width, height)),
        } = self.pending
        else {
            return false;
        };
        let logical = Some(Rect {
            x,
            y,
            width,
            height,
        });
        let changed = self.logical != logical;
        self.logical = logical;
        changed
    }
}

/// Every bound output, by registry name.
#[derive(Debug, Default)]
pub struct Outputs {
    by_global: HashMap<u32, Output>,
    manager: Option<ZxdgOutputManagerV1>,
}

impl Outputs {
    /// Binds the output with registry name `global`.
    pub fn add<State>(
        &mut self,
        registry: &WlRegistry,
        global: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) where
        State: Dispatch<WlOutput, OutputGlobal> + Dispatch<ZxdgOutputV1, OutputGlobal> + 'static,
    {
        let data = OutputGlobal(global);
        let proxy: WlOutput = registry.bind(global, version.min(OUTPUT_NAME_SINCE), qh, data);
        let xdg = self
            .manager
            .as_ref()
            .map(|manager| manager.get_xdg_output(&proxy, qh, data));
        self.by_global.insert(
            global,
            Output {
                proxy,
                xdg,
                name: None,
                pending: Pending::default(),
                logical: None,
            },
        );
    }

    /// Binds the xdg-output manager and describes every output with it.
    pub fn add_manager<State>(
        &mut self,
        registry: &WlRegistry,
        global: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) where
        State: Dispatch<ZxdgOutputManagerV1, ()> + Dispatch<ZxdgOutputV1, OutputGlobal> + 'static,
    {
        if self.manager.is_some() {
            return;
        }
        let manager: ZxdgOutputManagerV1 =
            registry.bind(global, version.min(XDG_DONE_REPLACED_SINCE), qh, ());
        for (&global, output) in &mut self.by_global {
            output.xdg = Some(manager.get_xdg_output(&output.proxy, qh, OutputGlobal(global)));
        }
        self.manager = Some(manager);
    }

    /// Releases the output with registry name `global`; whether it was one.
    pub fn remove(&mut self, global: u32) -> bool {
        let Some(output) = self.by_global.remove(&global) else {
            return false;
        };
        if let Some(xdg) = output.xdg {
            xdg.destroy();
        }
        if output.proxy.version() >= OUTPUT_RELEASE_SINCE {
            output.proxy.release();
        }
        true
    }

    /// The registry name of a bound output.
    #[must_use]
    pub fn global_of(output: &WlOutput) -> Option<u32> {
        output.data::<OutputGlobal>().map(|global| global.0)
    }

    /// The names of the outputs among `globals` that have one.
    #[must_use]
    pub fn names(&self, globals: &BTreeSet<u32>) -> BTreeSet<String> {
        globals
            .iter()
            .filter_map(|global| self.by_global.get(global)?.name.clone())
            .collect()
    }

    /// The names of every output that has one.
    #[must_use]
    pub fn all_names(&self) -> BTreeSet<String> {
        self.by_global
            .values()
            .filter_map(|output| output.name.clone())
            .collect()
    }

    /// Each named output's logical rectangle.
    pub fn layout(&self) -> impl Iterator<Item = (&str, Rect)> {
        self.by_global
            .values()
            .filter_map(|output| Some((output.name.as_deref()?, output.logical?)))
    }

    fn get_mut(&mut self, global: u32) -> Option<&mut Output> {
        self.by_global.get_mut(&global)
    }
}

impl<State> Dispatch<WlOutput, OutputGlobal, State> for Outputs
where
    State: Dispatch<WlOutput, OutputGlobal> + OutputsHandler,
{
    fn event(
        state: &mut State,
        proxy: &WlOutput,
        event: wl_output::Event,
        &OutputGlobal(global): &OutputGlobal,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(output) = state.outputs().get_mut(global) else {
            return;
        };
        let changed = match event {
            wl_output::Event::Name { name } => {
                output.name = Some(name);
                true
            }
            wl_output::Event::Done if proxy.version() >= OUTPUT_DONE_SINCE => output.apply(),
            _ => false,
        };
        if changed {
            state.outputs_changed();
        }
    }
}

impl<State> Dispatch<ZxdgOutputV1, OutputGlobal, State> for Outputs
where
    State: Dispatch<ZxdgOutputV1, OutputGlobal> + OutputsHandler,
{
    fn event(
        state: &mut State,
        xdg: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        &OutputGlobal(global): &OutputGlobal,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        let Some(output) = state.outputs().get_mut(global) else {
            return;
        };
        // Where neither `done` is sent, each event applies at once.
        let immediate =
            output.proxy.version() < OUTPUT_DONE_SINCE && xdg.version() >= XDG_DONE_REPLACED_SINCE;
        let changed = match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.pending.position = Some((x, y));
                immediate && output.apply()
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.pending.size = Some((width, height));
                immediate && output.apply()
            }
            zxdg_output_v1::Event::Name { name } => {
                let first = output.name.is_none();
                output.name.get_or_insert(name);
                first
            }
            zxdg_output_v1::Event::Done => output.apply(),
            _ => false,
        };
        if changed {
            state.outputs_changed();
        }
    }
}

impl<State> Dispatch<ZxdgOutputManagerV1, (), State> for Outputs
where
    State: Dispatch<ZxdgOutputManagerV1, ()>,
{
    fn event(
        _: &mut State,
        _: &ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(interface: &str, version: u32) -> Global {
        Global {
            name: 1,
            interface: interface.to_owned(),
            version,
        }
    }

    #[test]
    fn names_come_from_new_enough_outputs_or_xdg_output() {
        let wl = |version| global("wl_output", version);
        let xdg = |version| global("zxdg_output_manager_v1", version);
        assert!(names_available(&[wl(4), wl(4)]));
        assert!(
            !names_available(&[wl(4), wl(3)]),
            "one output would be nameless"
        );
        assert!(names_available(&[wl(3), xdg(2)]));
        assert!(!names_available(&[wl(3), xdg(1)]));
        assert!(
            names_available(&[xdg(3)]),
            "outputs plugged in later get names"
        );
        assert!(!names_available(&[]));
    }

    #[test]
    fn the_layout_needs_xdg_output() {
        assert!(layout_available(&[global("zxdg_output_manager_v1", 1)]));
        assert!(!layout_available(&[global("wl_output", 4)]));
    }
}
