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

/// The panel, following `live` until it closes.
#[component]
pub(crate) fn panel(live: LiveSettings<PanelSettings>) -> NodeHandle {
    // Where the window was made.
    let mut placed = Placement::of(&live.initial);
    let settings = live.into_signal();
    create_context(settings);
    let placement = Memo::new(move || Placement::of(&settings.get()));
    Effect::new(move || {
        let wanted = placement.get();
        if wanted != placed {
            surface::place(wanted);
            placed = wanted;
        }
    });
    rsx! {
        Bar {}
    }
}
