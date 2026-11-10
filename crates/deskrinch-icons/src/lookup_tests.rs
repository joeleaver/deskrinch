//! The lookup over indexes built in memory: no file is read, so these also
//! show a lookup touches none.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use super::*;
use crate::candidates::names;
use crate::index::{Indexed, Location};
use crate::theme::Theme;
use crate::theme::tests::BIRCH;

/// The base directories of every fixture: `/b0` before `/b1`.
const BASES: [&str; 2] = ["/b0", "/b1"];

/// A theme fixture: its name, its `index.theme`, and its icon files as
/// `(directory, base, file name)`.
type ThemeFixture<'a> = (&'a str, &'a str, &'a [(&'a str, usize, &'a str)]);

/// An index for lookups in `theme`, from `themes`: each a name, its
/// `index.theme`, and its icon files as `(directory, base, file name)`;
/// and the unthemed icon files as `(base, file name)`.
fn index(theme: &str, themes: &[ThemeFixture<'_>], unthemed: &[(usize, &str)]) -> Index {
    let mut indexed = HashMap::new();
    for (name, text, files) in themes {
        let theme = Theme::parse(name, text.as_bytes(), &mut Vec::new()).unwrap();
        let mut icons: HashMap<String, Vec<Location>> = HashMap::new();
        for (dir, base, file) in *files {
            let directory = theme
                .directories
                .iter()
                .position(|d| d.path == *dir)
                .unwrap();
            let (stem, format) = Format::split(file).unwrap();
            icons.entry(stem.into()).or_default().push(Location {
                directory,
                base: *base,
                format,
            });
        }
        for at in icons.values_mut() {
            at.sort();
        }
        indexed.insert((*name).to_owned(), Indexed { theme, icons });
    }
    let mut loose: HashMap<String, Vec<Location>> = HashMap::new();
    for (base, file) in unthemed {
        let (stem, format) = Format::split(file).unwrap();
        loose.entry(stem.into()).or_default().push(Location {
            directory: 0,
            base: *base,
            format,
        });
    }
    for at in loose.values_mut() {
        at.sort();
    }
    Index {
        search_path: BASES.iter().map(PathBuf::from).collect(),
        theme: theme.into(),
        consulted: Vec::new(),
        themes: indexed,
        unthemed: loose,
        problems: Vec::new(),
    }
}

/// A theme with `directories` (each `name:size:type`) and `inherits`.
fn theme_text(inherits: &str, directories: &[&str]) -> String {
    let names: Vec<&str> = directories
        .iter()
        .map(|d| d.split(':').next().unwrap())
        .collect();
    let mut text = format!(
        "[Icon Theme]\nName=T\nInherits={inherits}\nDirectories={}\n",
        names.join(",")
    );
    for d in directories {
        let parts: Vec<&str> = d.split(':').collect();
        write!(
            text,
            "[{}]\nSize={}\nType={}\n",
            parts[0], parts[1], parts[2]
        )
        .unwrap();
    }
    text
}

fn path(index: &Index, request: Request<'_>) -> String {
    index.lookup(&request).unwrap().path.display().to_string()
}

#[test]
fn finds_the_exact_size_in_the_specifications_example() {
    let files: &[(&str, usize, &str)] = &[
        ("48x48/apps", 0, "mozilla.png"),
        ("32x32/apps", 0, "mozilla.png"),
        ("48x48@2/apps", 0, "mozilla.png"),
        ("scalable/apps", 0, "mozilla.svg"),
    ];
    let birch = index("birch", &[("birch", BIRCH, files)], &[]);
    let at = |size, scale| path(&birch, Request::new("mozilla", size).at_scale(scale));
    assert_eq!(at(48, 1), "/b0/birch/48x48/apps/mozilla.png");
    assert_eq!(at(32, 1), "/b0/birch/32x32/apps/mozilla.png");
    assert_eq!(at(48, 2), "/b0/birch/48x48@2/apps/mozilla.png");
    // No fixed directory has 64; the scalable one serves 1 to 256.
    assert_eq!(at(64, 1), "/b0/birch/scalable/apps/mozilla.svg");
    let icon = birch.lookup(&Request::new("mozilla", 64)).unwrap();
    assert_eq!(icon.format, Format::Svg);
}

#[test]
fn prefers_directory_then_base_then_format_order() {
    let text = theme_text("", &["a:48:Fixed", "b:48:Fixed"]);
    let files: &[(&str, usize, &str)] = &[
        ("b", 0, "x.png"),
        ("a", 1, "x.svg"),
        ("a", 1, "x.png"),
        ("a", 1, "x.xpm"),
    ];
    let t = index("t", &[("t", &text, files)], &[]);
    // The first directory wins over the first base directory.
    assert_eq!(path(&t, Request::new("x", 48)), "/b1/t/a/x.png");
    let both: &[(&str, usize, &str)] = &[("a", 1, "x.png"), ("a", 0, "x.svg")];
    let t = index("t", &[("t", &text, both)], &[]);
    assert_eq!(path(&t, Request::new("x", 48)), "/b0/t/a/x.svg");
}

#[test]
fn takes_the_closest_size_the_first_on_a_tie() {
    let text = theme_text(
        "",
        &["16:16:Fixed", "24:24:Fixed", "40:40:Fixed", "64:64:Fixed"],
    );
    let files: &[(&str, usize, &str)] = &[
        ("16", 0, "x.png"),
        ("24", 0, "x.png"),
        ("40", 0, "x.png"),
        ("64", 0, "x.png"),
    ];
    let t = index("t", &[("t", &text, files)], &[]);
    assert_eq!(path(&t, Request::new("x", 22)), "/b0/t/24/x.png");
    assert_eq!(
        path(&t, Request::new("x", 52)),
        "/b0/t/40/x.png",
        "12 from both: the first"
    );
    assert_eq!(path(&t, Request::new("x", 1000)), "/b0/t/64/x.png");
    // At scale 2, 32 logical pixels are 64 device pixels.
    assert_eq!(
        path(&t, Request::new("x", 32).at_scale(2)),
        "/b0/t/64/x.png"
    );
}

#[test]
fn looks_in_parents_then_hicolor_then_the_base_directories() {
    let child = theme_text("parent", &["48:48:Fixed"]);
    let parent = theme_text("", &["48:48:Fixed"]);
    let hicolor = theme_text("", &["48:48:Fixed"]);
    let t = index(
        "child",
        &[
            ("child", &child, &[("48", 0, "own.png")]),
            (
                "parent",
                &parent,
                &[("48", 0, "inherited.png"), ("48", 0, "own.png")],
            ),
            (HICOLOR, &hicolor, &[("48", 1, "fallback.png")]),
        ],
        &[(1, "loose.xpm"), (0, "loose.png"), (0, "fallback.png")],
    );
    assert_eq!(path(&t, Request::new("own", 48)), "/b0/child/48/own.png");
    assert_eq!(
        path(&t, Request::new("inherited", 48)),
        "/b0/parent/48/inherited.png"
    );
    assert_eq!(
        path(&t, Request::new("fallback", 48)),
        "/b1/hicolor/48/fallback.png"
    );
    assert_eq!(path(&t, Request::new("loose", 48)), "/b0/loose.png");
    assert_eq!(
        t.lookup(&Request::new("absent", 48)),
        Err(LookupError::NotFound {
            name: "absent".into()
        })
    );
}

#[test]
fn falls_back_to_generic_names_theme_by_theme() {
    let child = theme_text("parent", &["48:48:Fixed"]);
    let parent = theme_text("", &["48:48:Fixed"]);
    let t = index(
        "child",
        &[
            ("child", &child, &[("48", 0, "input-mouse.png")]),
            (
                "parent",
                &parent,
                &[("48", 0, "input-mouse-usb.png"), ("48", 0, "input.png")],
            ),
        ],
        &[],
    );
    let request = Request::new("input-mouse-usb", 48);
    // Without the fallback, the exact name, wherever it is.
    assert_eq!(path(&t, request), "/b0/parent/48/input-mouse-usb.png");
    // With it, the theme's own more generic icon wins over its parent's
    // more specific one, as FindBestIcon has it.
    assert_eq!(
        path(&t, request.with_generic_fallback()),
        "/b0/child/48/input-mouse.png"
    );
    let unknown = Request::new("input-tablet-pen", 48);
    assert!(t.lookup(&unknown).is_err());
    assert_eq!(
        path(&t, unknown.with_generic_fallback()),
        "/b0/parent/48/input.png"
    );
    assert_eq!(names("a-b-c", true), ["a-b-c", "a-b", "a"]);
    assert_eq!(names("a-b-c", false), ["a-b-c"]);
    assert_eq!(names("-a", true), ["-a"]);
}

#[test]
fn prefers_symbolic_icons_at_every_level_for_a_symbolic_name() {
    assert_eq!(
        names("a-b-symbolic", true),
        ["a-b-symbolic", "a-symbolic", "a-b", "a"]
    );
    assert_eq!(names("symbolic", true), ["symbolic"]);
    assert_eq!(names("-symbolic", true), ["-symbolic"]);
    let text = theme_text("", &["48:48:Fixed"]);
    let t = index(
        "t",
        &[(
            "t",
            &text,
            &[
                ("48", 0, "audio-volume.png"),
                ("48", 0, "audio-symbolic.svg"),
            ],
        )],
        &[],
    );
    let request = Request::new("audio-volume-symbolic", 48).with_generic_fallback();
    assert_eq!(path(&t, request), "/b0/t/48/audio-symbolic.svg");
}

#[test]
fn gives_every_candidate_best_first() {
    let child = theme_text(
        "",
        &["16:16:Fixed", "48:48:Fixed", "32:32:Fixed", "64:64:Fixed"],
    );
    let hicolor = theme_text("", &["48:48:Fixed"]);
    let t = index(
        "child",
        &[
            (
                "child",
                &child,
                &[
                    ("64", 0, "x.png"),
                    ("16", 0, "x.png"),
                    ("32", 0, "x.png"),
                    ("48", 1, "x.png"),
                    ("48", 0, "x.svg"),
                ],
            ),
            (HICOLOR, &hicolor, &[("48", 0, "x.png")]),
        ],
        &[(1, "x.png"), (0, "x.xpm")],
    );
    let all: Vec<String> = t
        .candidates(&Request::new("x", 48))
        .unwrap()
        .map(|icon| icon.path.display().to_string())
        .collect();
    assert_eq!(
        all,
        [
            // The exact matches: base directory first, then format.
            "/b0/child/48/x.svg",
            "/b1/child/48/x.png",
            // The other sizes by distance, 32 and 64 tied: directory order.
            "/b0/child/32/x.png",
            "/b0/child/64/x.png",
            "/b0/child/16/x.png",
            "/b0/hicolor/48/x.png",
            "/b0/x.xpm",
            "/b1/x.png",
        ]
    );
}

#[test]
fn survives_a_cycle_in_inherits_and_a_missing_hicolor() {
    let a = theme_text("b", &["48:48:Fixed"]);
    let b = theme_text("a", &["48:48:Fixed"]);
    let t = index(
        "a",
        &[("a", &a, &[]), ("b", &b, &[("48", 0, "in-b.png")])],
        &[(0, "loose.png")],
    );
    assert_eq!(path(&t, Request::new("in-b", 48)), "/b0/b/48/in-b.png");
    // Not found anywhere: the cycle ends the search; there is no hicolor.
    assert!(matches!(
        t.lookup(&Request::new("absent", 48)),
        Err(LookupError::NotFound { .. })
    ));
    assert_eq!(path(&t, Request::new("loose", 48)), "/b0/loose.png");
    // A theme that is not in the index at all: straight to the fallback.
    let none = index("missing", &[], &[(0, "loose.png")]);
    assert_eq!(path(&none, Request::new("loose", 48)), "/b0/loose.png");
}

#[test]
fn refuses_what_is_not_a_name() {
    let t = index("t", &[], &[]);
    for (name, size, scale) in [
        ("", 48, 1),
        ("a/b", 48, 1),
        ("../x", 48, 1),
        ("x", 0, 1),
        ("x", 48, 0),
    ] {
        let request = Request::new(name, size).at_scale(scale);
        assert!(
            matches!(t.lookup(&request), Err(LookupError::InvalidRequest { .. })),
            "{name:?} {size} {scale}"
        );
    }
    assert_eq!(
        t.lookup(&Request::new("/usr/share/pixmaps/x.png", 48)),
        Err(LookupError::Path("/usr/share/pixmaps/x.png".into()))
    );
}
