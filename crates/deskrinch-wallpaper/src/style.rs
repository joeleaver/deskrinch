//! What each output's surface draws, as CSS: the colour, and over it the
//! picture fitted as the settings say. rinch scales the picture to each
//! output when it paints, from the one decoded copy it keeps.

use std::fmt::Write as _;

use deskrinch_settings::wallpaper::{Fit, WallpaperSettings};
use deskrinch_theme::look::DESKTOP_BACKGROUND;

/// The style of the element that fills an output's surface, for
/// `settings`, with the picture at `image` (an image source rinch loads),
/// or none.
pub(crate) fn css(settings: &WallpaperSettings, image: Option<&str>) -> String {
    let color = settings
        .color
        .map_or_else(|| DESKTOP_BACKGROUND.to_owned(), |color| color.to_string());
    let mut style = format!("width: 100%; height: 100%; background-color: {color};");
    if let Some(image) = image {
        let (size, position, repeat) = match settings.fit {
            Fit::Cover => ("cover", "center", "no-repeat"),
            Fit::Contain => ("contain", "center", "no-repeat"),
            Fit::Stretch => ("100% 100%", "center", "no-repeat"),
            Fit::Center => ("auto", "center", "no-repeat"),
            Fit::Tile => ("auto", "top left", "repeat"),
        };
        // Writing to a `String` cannot fail.
        let _ = write!(
            style,
            " background-image: url(\"{}\"); background-size: {size}; \
             background-position: {position}; background-repeat: {repeat};",
            css_string(image)
        );
    }
    style
}

/// `text` inside a CSS double-quoted string: its backslashes and quotes
/// escaped, and a line break, which a CSS string cannot hold, as its
/// escape.
fn css_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '"' => {
                escaped.push('\\');
                escaped.push(c);
            }
            '\n' => escaped.push_str("\\a "),
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use deskrinch_settings::wallpaper::Color;

    use super::*;

    fn settings(fit: Fit) -> WallpaperSettings {
        WallpaperSettings {
            color: Some(Color {
                red: 0x12,
                green: 0x34,
                blue: 0x56,
            }),
            image: None,
            fit,
        }
    }

    #[test]
    fn without_a_colour_it_is_the_looks_background() {
        let style = css(&WallpaperSettings::default(), None);
        assert!(
            style.contains(&format!("background-color: {DESKTOP_BACKGROUND};")),
            "{style}"
        );
        assert!(!style.contains("background-image"), "{style}");
    }

    #[test]
    fn the_colour_alone_fills_the_output() {
        let style = css(&settings(Fit::Cover), None);
        assert_eq!(
            style,
            "width: 100%; height: 100%; background-color: #123456;"
        );
    }

    #[test]
    fn each_fit_is_its_css() {
        let image = Some("wallpaper:/p/a.png");
        for (fit, expected) in [
            (
                Fit::Cover,
                "background-size: cover; background-position: center; background-repeat: no-repeat;",
            ),
            (
                Fit::Contain,
                "background-size: contain; background-position: center; background-repeat: no-repeat;",
            ),
            (
                Fit::Stretch,
                "background-size: 100% 100%; background-position: center; background-repeat: no-repeat;",
            ),
            (
                Fit::Center,
                "background-size: auto; background-position: center; background-repeat: no-repeat;",
            ),
            (
                Fit::Tile,
                "background-size: auto; background-position: top left; background-repeat: repeat;",
            ),
        ] {
            let style = css(&settings(fit), image);
            assert!(style.contains(expected), "{fit:?}: {style}");
            assert!(
                style.contains("background-image: url(\"wallpaper:/p/a.png\");"),
                "{style}"
            );
            assert!(style.contains("background-color: #123456;"), "{style}");
        }
    }

    #[test]
    fn a_path_is_quoted_whatever_it_holds() {
        assert_eq!(css_string(r#"/a "b" \c"#), r#"/a \"b\" \\c"#);
        assert_eq!(css_string("/a\nb"), "/a\\a b");
        assert_eq!(css_string("/a (b).png"), "/a (b).png");
    }
}
