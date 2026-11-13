//! One window's button: its application's icon and its title, marked as
//! the model says; and the pointer's presses and releases on it.

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::{Applications, icon_scale, path_of};
use deskrinch_desktop_entries::Locale;
use deskrinch_icons::Index;
use deskrinch_taskbar::{Button, Form, Input, PointerButton};
use deskrinch_window_management::WindowId;
use rinch::prelude::*;

use crate::widgets::taskbar::follow::{Buttons, Following};

/// What the buttons' icons come from, which the panel provides as a
/// context: the installed applications, the icon theme's index, and the
/// user's locales, which an entry's `Icon` may be translated into.
#[derive(Clone)]
pub(crate) struct AppIcons {
    pub(crate) applications: Signal<Arc<Applications>>,
    pub(crate) index: Signal<Option<Arc<Index>>>,
    pub(crate) locales: Arc<Vec<Locale>>,
}

/// The sizes a button comes in, in logical pixels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ButtonSizes {
    /// The icon's side.
    pub(crate) icon: u32,
    /// The narrowest a button with its title is drawn.
    pub(crate) titled_min: f32,
    /// The widest a button with its title is drawn.
    pub(crate) titled_max: f32,
    /// Around the icon, inside the button.
    pub(crate) padding: f32,
    /// Between two buttons.
    pub(crate) gap: f32,
    /// The count of the windows left out.
    pub(crate) count_width: f32,
}

impl ButtonSizes {
    /// The width of a button with its icon alone.
    pub(crate) fn icon_only(&self) -> f32 {
        f32::from(u16::try_from(self.icon).unwrap_or(u16::MAX)) + 2.0 * self.padding
    }
}

/// The button of the window `id`, drawn from `buttons` as it changes, in
/// the `form` the room allows.
pub(crate) fn button_view(
    __scope: &mut RenderScope,
    id: WindowId,
    buttons: Signal<Buttons>,
    form: Memo<Form>,
    following: &Rc<Following>,
    sizes: ButtonSizes,
) -> NodeHandle {
    let button =
        Memo::new(move || buttons.with(|all| all.iter().find(|button| button.id == id).cloned()));
    let icons: AppIcons = use_context();
    let icon = icon_view(__scope, button, icons, sizes.icon);
    let title = Memo::new(move || button.get().map(|b| b.title).unwrap_or_default());
    let titled = Memo::new(move || matches!(form.get(), Form::Titled { .. }));
    let style = Memo::new(move || style(button.get().as_ref(), form.get(), sizes));
    rsx! {
        div {
            class: "taskbar-button",
            style: {move || style.get()},
            onmousedown: pointer(following, Some(id), true),
            onmouseup: pointer(following, Some(id), false),
            {icon}
            if titled.get() {
                span {
                    style: "flex: 1 1 auto; min-width: 0; overflow: hidden; \
                            white-space: nowrap; text-overflow: ellipsis;",
                    {move || title.get()}
                }
            }
        }
    }
}

/// The button's style: its width for `form`, and its marks.
fn style(button: Option<&Button<WindowId>>, form: Form, sizes: ButtonSizes) -> String {
    let width = match form {
        Form::Titled { width } => width,
        Form::IconOnly => sizes.icon_only(),
    };
    let (active, minimized, attention) = button.map_or((false, false, false), |b| {
        (b.active, b.minimized, b.attention)
    });
    format!(
        "display: flex; flex-direction: row; align-items: center; flex: none; \
         box-sizing: border-box; width: {width}px; height: 100%; \
         gap: var(--rinch-spacing-xs); padding: 0 {pad}px; \
         border-radius: var(--rinch-radius-sm); \
         background: {background}; opacity: {opacity}; \
         border-bottom: 2px solid {attention};",
        pad = sizes.padding,
        background = if active {
            "var(--rinch-color-option-selected)"
        } else {
            "transparent"
        },
        opacity = if minimized { "0.55" } else { "1" },
        attention = if attention {
            "var(--rinch-color-filled)"
        } else {
            "transparent"
        },
    )
}

/// The button's icon: its application's, found by the window's application
/// id, else the generic program's; an empty square until the theme's index
/// is built, or when the theme has neither.
fn icon_view(
    __scope: &mut RenderScope,
    button: Memo<Option<Button<WindowId>>>,
    icons: AppIcons,
    size: u32,
) -> NodeHandle {
    let app_id = Memo::new(move || button.get().map(|b| b.app_id).unwrap_or_default());
    let AppIcons {
        applications,
        index,
        locales,
    } = icons;
    let path = Memo::new(move || {
        let index = index.get()?;
        let app_id = app_id.get();
        let scale = icon_scale(scale_factor());
        applications.with(|applications| {
            let icon = applications
                .for_app_id(&app_id)
                .and_then(|entry| entry.icon.as_ref())
                .map(|icon| icon.get(&locales).clone());
            path_of(&index, icon.as_ref(), size, scale)
        })
    });
    let square = format!("width: {size}px; height: {size}px; flex: none;");
    let image = square.clone();
    rsx! {
        div {
            style: square,
            if let Some(src) = path.get() {
                img { src: src, style: image.clone() }
            }
        }
    }
}

/// A press (`down`) or release handler for the primary and middle buttons,
/// on the button of `on`, or on the taskbar but no button.
pub(crate) fn pointer(
    following: &Rc<Following>,
    on: Option<WindowId>,
    down: bool,
) -> impl Fn() + 'static {
    let following = Rc::clone(following);
    move || {
        let button = match get_click_context().button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Middle => PointerButton::Middle,
            MouseButton::Right => return,
        };
        following.input(if down {
            Input::Press(on, button)
        } else {
            Input::Release(on, button)
        });
    }
}
