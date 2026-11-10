//! The panel's root: its settings as a signal, and the window following
//! them.
//!
//! The settings the panel started with made its window. Every later change
//! from the configuration arrives in a `Signal<PanelSettings>`, which the
//! widgets read through the context. A change of edge or height moves the
//! window ([`surface::place`]); anything else leaves it alone.

use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::bar::Bar;
use crate::config::Config;
use crate::surface;

/// The panel, following `config`, which it keeps running until it closes.
#[component]
pub(crate) fn panel(config: Config) -> NodeHandle {
    let settings = Signal::new(config.initial.clone());
    create_context(settings);
    let placement = Memo::new(move || settings.get().placement());
    let mut placed = config.initial.placement();
    Effect::new(move || {
        let wanted = placement.get();
        if wanted != placed {
            surface::place(wanted);
            placed = wanted;
        }
    });
    if let Some(newer) = config.connect(settings) {
        settings.set(newer);
    }
    rinch::reactive::on_cleanup(move || drop(config));
    rsx! {
        Bar {}
    }
}
