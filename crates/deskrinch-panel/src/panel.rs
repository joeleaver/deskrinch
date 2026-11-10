//! The panel's root: its settings as a signal, and the window following
//! them.
//!
//! The settings the panel started with made its window. Every later change
//! from the configuration arrives in a `Signal<PanelSettings>`, which the
//! widgets read through the context. A change of edge or height moves the
//! window ([`surface::place`]); anything else leaves it alone.

use deskrinch_component::LiveSettings;
use deskrinch_settings::panel::PanelSettings;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::bar::Bar;
use crate::surface::{self, Placement};

/// The panel, following `live`, which it keeps until it closes.
#[component]
pub(crate) fn panel(live: LiveSettings<PanelSettings>) -> NodeHandle {
    let settings = Signal::new(live.initial.clone());
    create_context(settings);
    let placement = Memo::new(move || Placement::of(&settings.get()));
    let mut placed = Placement::of(&live.initial);
    Effect::new(move || {
        let wanted = placement.get();
        if wanted != placed {
            surface::place(wanted);
            placed = wanted;
        }
    });
    if let Some(newer) = live.connect(settings) {
        settings.set(newer);
    }
    rinch::reactive::on_cleanup(move || drop(live));
    rsx! {
        Bar {}
    }
}
