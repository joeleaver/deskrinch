//! The pure model on fixtures: layering, merging, diagnostics, unknown keys,
//! last good versions and the effective listing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    DefaultsError, Diagnostic, Location, Problem, Resolved, Section, Severity, Snapshot,
    ValueSource,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Edge {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Clock {
    format: String,
    seconds: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Panel {
    edge: Edge,
    height: u32,
    widgets: Vec<String>,
    clock: Clock,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            edge: Edge::Top,
            height: 32,
            widgets: vec!["clock".into(), "tray".into()],
            clock: Clock {
                format: "%H:%M".into(),
                seconds: false,
            },
        }
    }
}

const LOW: &str = "/etc/xdg/deskrinch/config.toml";
const HIGH: &str = "/etc/xdg/vendor/deskrinch/config.toml";
const USER: &str = "/home/u/.config/deskrinch/config.toml";

/// A snapshot of the three layers, lowest first.
fn snapshot(low: Option<&str>, high: Option<&str>, user: Option<&str>) -> Snapshot {
    Snapshot::from_texts(
        [(LOW, low), (HIGH, high), (USER, user)]
            .map(|(path, text)| (PathBuf::from(path), text.map(str::to_owned))),
    )
}

fn section() -> Section<Panel> {
    Section::new("panel").unwrap()
}

fn resolve(low: Option<&str>, high: Option<&str>, user: Option<&str>) -> Resolved<Panel> {
    section().resolve(&snapshot(low, high, user))
}

fn only(diagnostics: &[Diagnostic]) -> &Diagnostic {
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    &diagnostics[0]
}

fn at(line: usize, column: usize) -> Location {
    Location { line, column }
}

#[test]
fn uses_the_defaults_without_files() {
    let resolved = resolve(None, None, None);
    assert_eq!(resolved.settings, Panel::default());
    assert_eq!(resolved.diagnostics, Vec::<Diagnostic>::new());
    assert!(
        resolved
            .effective
            .values
            .iter()
            .all(|value| value.source == ValueSource::Default)
    );
}

#[test]
fn merges_tables_key_by_key_from_the_lowest_layer() {
    let resolved = resolve(
        Some("[panel]\nheight = 40\n[panel.clock]\nformat = '%H'\n"),
        Some("[panel.clock]\nseconds = true\n"),
        Some("[panel]\nedge = 'bottom'\nheight = 50\n"),
    );
    assert_eq!(resolved.diagnostics, Vec::<Diagnostic>::new());
    let expected = Panel {
        edge: Edge::Bottom,
        height: 50,
        clock: Clock {
            format: "%H".into(),
            seconds: true,
        },
        ..Panel::default()
    };
    assert_eq!(resolved.settings, expected);
}

#[test]
fn replaces_arrays_whole() {
    let resolved = resolve(
        Some("[panel]\nwidgets = ['a', 'b', 'c']\n"),
        None,
        Some("panel.widgets = ['d']\n"),
    );
    assert_eq!(resolved.settings.widgets, ["d"]);
}

#[test]
fn ignores_other_components_sections() {
    let resolved = resolve(None, None, Some("[launcher]\nterminal = 'foot'\n"));
    assert_eq!(resolved.settings, Panel::default());
    assert_eq!(resolved.diagnostics, Vec::<Diagnostic>::new());
}

#[test]
fn skips_a_file_that_is_not_toml() {
    let resolved = resolve(
        Some("[panel]\nheight = 40\n"),
        None,
        Some("[panel]\nheight = = 3\n"),
    );
    assert_eq!(resolved.settings.height, 40);
    let diagnostic = only(&resolved.diagnostics);
    assert_eq!(diagnostic.file, Path::new(USER));
    assert_eq!(diagnostic.location.map(|l| l.line), Some(2));
    assert_eq!(diagnostic.severity(), Severity::Error);
    assert!(matches!(diagnostic.problem, Problem::Syntax { .. }));
}

#[test]
fn skips_a_file_with_a_value_of_the_wrong_type() {
    let resolved = resolve(
        Some("[panel]\nheight = 40\n"),
        None,
        Some("[panel]\nedge = 'bottom'\nheight = 'tall'\n"),
    );
    // The whole layer is skipped, not only the bad value.
    assert_eq!(resolved.settings.edge, Edge::Top);
    assert_eq!(resolved.settings.height, 40);
    let diagnostic = only(&resolved.diagnostics);
    assert_eq!(diagnostic.file, Path::new(USER));
    assert_eq!(diagnostic.location, Some(at(3, 10)));
    let Problem::Invalid {
        section,
        key,
        message,
    } = &diagnostic.problem
    else {
        panic!("{diagnostic:?}");
    };
    assert_eq!(
        (section.as_str(), key.as_deref()),
        ("panel", Some("height"))
    );
    assert!(message.contains("expected u32"), "{message}");
    assert_eq!(
        diagnostic.to_string(),
        format!("{USER}:3:10: error: panel.height: {message}")
    );
}

#[test]
fn reports_a_value_out_of_range_or_not_among_the_choices() {
    let negative = resolve(None, None, Some("[panel]\nheight = -1\n"));
    assert_eq!(only(&negative.diagnostics).location, Some(at(2, 10)));
    let unknown = resolve(None, None, Some("[panel]\nedge = 'left'\n"));
    let diagnostic = only(&unknown.diagnostics);
    assert_eq!(diagnostic.location, Some(at(2, 8)));
    assert!(diagnostic.to_string().contains("unknown variant `left`"));
}

#[test]
fn locates_an_error_inside_an_array_and_a_nested_table() {
    let resolved = resolve(None, None, Some("[panel]\nwidgets = ['a', 3]\n"));
    let diagnostic = only(&resolved.diagnostics);
    assert_eq!(diagnostic.location, Some(at(2, 17)));
    assert!(matches!(
        &diagnostic.problem,
        Problem::Invalid { key: Some(key), .. } if key == "widgets[1]"
    ));
    let nested = resolve(None, None, Some("[panel.clock]\nseconds = 'yes'\n"));
    let diagnostic = only(&nested.diagnostics);
    assert_eq!(diagnostic.location, Some(at(2, 11)));
    assert!(diagnostic.to_string().contains("panel.clock.seconds"));
}

#[test]
fn reports_a_section_that_is_not_a_table() {
    let resolved = resolve(None, None, Some("panel = 3\n"));
    assert_eq!(resolved.settings, Panel::default());
    let diagnostic = only(&resolved.diagnostics);
    assert_eq!(diagnostic.location, Some(at(1, 9)));
    assert_eq!(
        diagnostic.problem,
        Problem::NotATable {
            section: "panel".into(),
            found: "integer"
        }
    );
}

#[test]
fn warns_of_unknown_keys_where_each_file_writes_them() {
    let resolved = resolve(
        Some("[panel]\nheigth = 40\n"),
        None,
        Some("[panel]\nheight = 50\nheigth = 40\n[panel.clock]\nformt = 'x'\n"),
    );
    // The rest of each file applies.
    assert_eq!(resolved.settings.height, 50);
    let found: Vec<_> = resolved
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.file.to_str().unwrap(),
                d.location,
                d.severity(),
                d.to_string(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                LOW,
                Some(at(2, 1)),
                Severity::Warning,
                format!("{LOW}:2:1: warning: unknown key `panel.heigth`")
            ),
            (
                USER,
                Some(at(3, 1)),
                Severity::Warning,
                format!("{USER}:3:1: warning: unknown key `panel.heigth`")
            ),
            (
                USER,
                Some(at(5, 1)),
                Severity::Warning,
                format!("{USER}:5:1: warning: unknown key `panel.clock.formt`")
            ),
        ]
    );
    assert!(
        resolved
            .effective
            .values
            .iter()
            .all(|value| !value.key.contains("heigth"))
    );
}

#[test]
fn keeps_a_files_last_good_version_when_it_breaks() {
    let mut panel = section();
    let good = panel.resolve(&snapshot(None, None, Some("[panel]\nheight = 40\n")));
    assert_eq!(good.settings.height, 40);

    for broken in [
        "[panel]\nheight = \n",
        "[panel]\nheight = 'tall'\n",
        "panel = 1\n",
    ] {
        let resolved = panel.resolve(&snapshot(None, None, Some(broken)));
        assert_eq!(resolved.settings.height, 40, "{broken}");
        assert_eq!(only(&resolved.diagnostics).file, Path::new(USER));
        let height = resolved
            .effective
            .values
            .iter()
            .find(|value| value.key == "height")
            .unwrap();
        assert!(
            matches!(height.source, ValueSource::File { kept: true, .. }),
            "{height:?}"
        );
    }

    // The layers below still apply while the broken file is kept.
    let resolved = panel.resolve(&snapshot(
        Some("[panel]\nedge = 'bottom'\n"),
        None,
        Some("[panel]\nheight = \n"),
    ));
    assert_eq!(
        (resolved.settings.edge, resolved.settings.height),
        (Edge::Bottom, 40)
    );

    // Removing the file removes its layer, and its last good version.
    let removed = panel.resolve(&snapshot(None, None, None));
    assert_eq!(removed.settings, Panel::default());
    let broken = panel.resolve(&snapshot(None, None, Some("[panel]\nheight = \n")));
    assert_eq!(broken.settings, Panel::default());
}

#[test]
fn skips_a_file_broken_from_the_start() {
    let mut panel = section();
    let broken = panel.resolve(&snapshot(
        Some("[panel]\nheight = 40\n"),
        None,
        Some("[panel]\nheight = 'tall'\n"),
    ));
    assert_eq!(broken.settings.height, 40);
    let fixed = panel.resolve(&snapshot(None, None, Some("[panel]\nheight = 50\n")));
    assert_eq!(fixed.settings.height, 50);
    assert_eq!(fixed.diagnostics, Vec::<Diagnostic>::new());
}

#[test]
fn lists_effective_values_with_their_layers() {
    let resolved = resolve(
        Some("[panel]\nheight = 40\n"),
        None,
        Some("[panel.clock]\nformat = '%H'\n"),
    );
    assert_eq!(
        resolved.effective.to_string(),
        format!(
            "[panel]\n\
             clock.format = \"%H\"  # {USER}:2:10\n\
             clock.seconds = false  # default\n\
             edge = \"top\"  # default\n\
             height = 40  # {LOW}:2:10\n\
             widgets = [\"clock\", \"tray\"]  # default\n"
        )
    );
}

#[test]
fn rejects_defaults_that_are_not_a_table() {
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    struct Height(u32);
    assert!(matches!(
        Section::<Height>::new("panel"),
        Err(DefaultsError::NotATable { .. })
    ));
}
