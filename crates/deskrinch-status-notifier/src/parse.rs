//! From the properties as sent to an [`Item`]: strings cleaned and bounded,
//! names checked, pixmaps validated and decoded; and from a layout as sent
//! to a menu ([`menu`]).

use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_untrusted::markup;
use deskrinch_untrusted::text::{self, Lines};
use deskrinch_untrusted::{IconName, Image};

use crate::item::{Category, Icon, Item, ItemId, Status, ToolTip};
use crate::limits::Limits;
use crate::wire::{Properties, RawPixmap, RawToolTip};

/// The item `id` that `properties` describe, within `limits`.
pub(crate) fn item(id: ItemId, properties: Properties, limits: &Limits) -> Item {
    let Properties {
        category,
        id: app_id,
        title,
        status,
        icon_theme_path,
        menu,
        item_is_menu,
        icon_name,
        icon_pixmap,
        overlay_icon_name,
        overlay_icon_pixmap,
        attention_icon_name,
        attention_icon_pixmap,
        tool_tip,
    } = properties;
    let short = |s: Option<String>| {
        s.map_or_else(String::new, |s| {
            text::clean(&s, limits.max_text_bytes, Lines::Join)
        })
    };
    Item {
        id,
        app_id: short(app_id),
        category: category
            .as_deref()
            .map_or_else(Category::default, Category::parse),
        title: short(title),
        status: status
            .as_deref()
            .map_or_else(Status::default, Status::parse),
        icon: icon(icon_name.as_deref(), icon_pixmap, limits),
        overlay: icon(overlay_icon_name.as_deref(), overlay_icon_pixmap, limits),
        attention: icon(
            attention_icon_name.as_deref(),
            attention_icon_pixmap,
            limits,
        ),
        icon_theme_path: icon_theme_path.and_then(|p| theme_path(&p, limits)),
        tooltip: tool_tip.and_then(|t| tooltip(t, limits)),
        item_is_menu: item_is_menu.unwrap_or(false),
        menu: menu.map(|p| p.as_str().to_owned()).filter(|p| p != "/"),
    }
}

fn icon(name: Option<&str>, pixmaps: Option<Vec<RawPixmap>>, limits: &Limits) -> Icon {
    let mut pixmaps: Vec<Arc<Image>> = pixmaps
        .unwrap_or_default()
        .into_iter()
        .take(limits.max_pixmaps)
        .filter_map(|(width, height, argb)| decode(width, height, &argb, limits.max_image_side))
        .map(Arc::new)
        .collect();
    pixmaps.sort_by_key(|p| u64::from(p.width()) * u64::from(p.height()));
    Icon {
        name: name
            .filter(|n| n.len() <= limits.max_reference_bytes)
            .and_then(IconName::parse),
        pixmaps,
    }
}

fn tooltip(raw: RawToolTip, limits: &Limits) -> Option<ToolTip> {
    let (name, pixmaps, title, description) = raw;
    let tooltip = ToolTip {
        icon: icon(Some(&name), Some(pixmaps), limits),
        title: text::clean(&title, limits.max_text_bytes, Lines::Join),
        description: markup::parse(
            &description,
            markup::Options {
                hyperlinks: false,
                max_markup_bytes: limits.max_description_markup_bytes,
                max_text_bytes: limits.max_description_bytes,
            },
        )
        .plain_text(),
    };
    let empty =
        tooltip.icon.is_empty() && tooltip.title.is_empty() && tooltip.description.is_empty();
    (!empty).then_some(tooltip)
}

/// An absolute directory path, with no NUL or other control characters.
pub(crate) fn theme_path(path: &str, limits: &Limits) -> Option<PathBuf> {
    let valid = path.starts_with('/')
        && path.len() <= limits.max_reference_bytes
        && text::clean(path, path.len(), Lines::Join) == path;
    valid.then(|| PathBuf::from(path))
}

/// Validates one ARGB32 pixmap and decodes it to RGBA, refusing one wider or
/// taller than `max_side`, or whose data is not exactly `width × height`
/// pixels. The size is checked before anything is allocated.
pub(crate) fn decode(width: i32, height: i32, argb: &[u8], max_side: u32) -> Option<Image> {
    let side = |v: i32| u32::try_from(v).ok().filter(|v| (1..=max_side).contains(v));
    let (width, height) = (side(width)?, side(height)?);
    let len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if argb.len() != len {
        return None;
    }
    let mut rgba = Vec::with_capacity(len);
    for &[a, r, g, b] in argb.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    Image::new(width, height, rgba)
}

pub(crate) mod menu;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod props;
#[cfg(test)]
mod tests;
