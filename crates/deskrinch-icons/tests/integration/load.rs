//! Building the index from fixture directories.

use std::os::unix::fs::symlink;

use deskrinch_icons::{Format, HICOLOR, LookupError, ProblemKind, Request, SearchPath};
use deskrinch_xdg::BaseDirs;

use crate::support::{Fixture, found, theme_text, write};

#[test]
fn takes_index_theme_from_the_first_base_and_icons_from_all() {
    let f = Fixture::new();
    // The user's copy of hicolor has icons but, as usual, no index.theme.
    f.theme(
        "b1",
        HICOLOR,
        &theme_text("", &["48:48:Fixed", "64:64:Fixed"]),
    );
    f.icon("b0", HICOLOR, "48", "user.png");
    f.icon("b1", HICOLOR, "48", "system.png");
    f.icon("b1", HICOLOR, "48", "user.png");
    // A theme whose first index.theme lists only one directory.
    f.theme("b0", "t", &theme_text("", &["48:48:Fixed"]));
    f.theme("b1", "t", &theme_text("", &["48:48:Fixed", "64:64:Fixed"]));
    f.icon("b1", "t", "64", "only-in-64.png");
    let index = f.load("t");
    assert_eq!(index.problems(), []);
    assert_eq!(
        found(&index, "user"),
        Some(f.base("b0").join("hicolor/48/user.png"))
    );
    assert_eq!(
        found(&index, "system"),
        Some(f.base("b1").join("hicolor/48/system.png"))
    );
    // 64 is not one of t's directories: its first index.theme says so.
    assert_eq!(found(&index, "only-in-64"), None);
    assert_eq!(index.consulted(), ["t", HICOLOR]);
}

#[test]
fn finds_scaled_and_scalable_icons_from_the_example_theme() {
    let f = Fixture::new();
    let birch = "[Icon Theme]\nName=Birch\nInherits=wood,default\n\
                 Directories=48x48/apps,scalable/apps\nScaledDirectories=48x48@2/apps\n\
                 [48x48/apps]\nSize=48\nType=Fixed\n\
                 [48x48@2/apps]\nSize=48\nScale=2\nType=Fixed\n\
                 [scalable/apps]\nSize=48\nType=Scalable\nMinSize=1\nMaxSize=256\n";
    f.theme("b1", "birch", birch);
    f.icon("b1", "birch", "48x48/apps", "mozilla.png");
    f.icon("b1", "birch", "48x48@2/apps", "mozilla.png");
    f.icon("b1", "birch", "scalable/apps", "mozilla.svg");
    let index = f.load("birch");
    let at = |size, scale| {
        let icon = index
            .lookup(&Request::new("mozilla", size).at_scale(scale))
            .unwrap();
        (
            icon.path
                .strip_prefix(f.base("b1"))
                .unwrap()
                .display()
                .to_string(),
            icon.format,
        )
    };
    assert_eq!(
        at(48, 1),
        ("birch/48x48/apps/mozilla.png".into(), Format::Png)
    );
    assert_eq!(
        at(48, 2),
        ("birch/48x48@2/apps/mozilla.png".into(), Format::Png)
    );
    assert_eq!(
        at(128, 1),
        ("birch/scalable/apps/mozilla.svg".into(), Format::Svg)
    );
    // `wood` and `default` are not installed, nor is hicolor: each noted.
    let missing: Vec<&str> = index
        .problems()
        .iter()
        .filter(|p| p.kind == ProblemKind::Missing)
        .map(|p| p.theme.as_str())
        .collect();
    assert_eq!(missing, ["wood", "default", HICOLOR]);
}

#[test]
fn skips_a_malformed_theme_for_hicolor() {
    let f = Fixture::new();
    f.theme(
        "b0",
        "broken",
        "[Icon Theme]\nName=Broken\nthis line is no key\n",
    );
    f.theme("b0", HICOLOR, &theme_text("", &["48:48:Fixed"]));
    f.icon("b0", "broken", "48", "x.png");
    f.icon("b0", HICOLOR, "48", "x.png");
    let index = f.load("broken");
    assert!(index.installed("broken").is_none());
    assert_eq!(
        found(&index, "x"),
        Some(f.base("b0").join("hicolor/48/x.png"))
    );
    let problem = &index.problems()[0];
    assert_eq!(problem.theme, "broken");
    assert_eq!(problem.path, Some(f.base("b0").join("broken/index.theme")));
    assert!(
        matches!(problem.kind, ProblemKind::Syntax(_)),
        "{problem:?}"
    );
}

#[test]
fn survives_a_cycle_and_a_missing_hicolor() {
    let f = Fixture::new();
    f.theme("b0", "a", &theme_text("b", &["48:48:Fixed"]));
    f.theme("b0", "b", &theme_text("c", &["48:48:Fixed"]));
    f.theme("b0", "c", &theme_text("a", &["48:48:Fixed"]));
    f.icon("b0", "c", "48", "in-c.png");
    let index = f.load("a");
    assert_eq!(
        found(&index, "in-c"),
        Some(f.base("b0").join("c/48/in-c.png"))
    );
    assert!(matches!(
        index.lookup(&Request::new("absent", 48)),
        Err(LookupError::NotFound { .. })
    ));
    let kinds: Vec<String> = index
        .problems()
        .iter()
        .map(|p| p.kind.to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "the theme is not installed (no index.theme in any base directory)",
            "Inherits forms a cycle: a → b → c",
        ]
    );
    assert_eq!(index.problems()[0].theme, HICOLOR);
}

#[test]
fn falls_back_to_the_base_directories_and_pixmaps() {
    let f = Fixture::new();
    f.icon("b1", "", "", "loose.svg");
    f.icon("pixmaps", "", "", "loose.png");
    f.icon("pixmaps", "", "", "only-pixmap.xpm");
    // An icon in a subdirectory of a base directory is in no theme.
    f.icon("b0", "not-a-theme", "", "deep.png");
    let index = f.load("none");
    assert_eq!(found(&index, "loose"), Some(f.base("b1").join("loose.svg")));
    assert_eq!(
        found(&index, "only-pixmap"),
        Some(f.base("pixmaps").join("only-pixmap.xpm"))
    );
    assert_eq!(found(&index, "deep"), None);
}

#[test]
fn lists_links_and_finds_past_broken_ones() {
    let f = Fixture::new();
    f.theme("b0", "t", &theme_text("", &["48:48:Fixed", "64:64:Fixed"]));
    f.theme("b0", HICOLOR, &theme_text("", &["48:48:Fixed"]));
    let target = f.icon("b0", HICOLOR, "48", "real.png");
    let alias = f.base("b0").join("t/48/alias.png");
    std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
    symlink(&target, &alias).unwrap();
    // The theme's best `x` is a broken link, its next a link to a
    // directory; then a real file at another size, then hicolor's.
    let broken = f.base("b0").join("t/48/x.png");
    symlink(f.root.join("nowhere.png"), &broken).unwrap();
    symlink(f.base("b0"), f.base("b0").join("t/48/x.svg")).unwrap();
    let other_size = f.icon("b0", "t", "64", "x.png");
    f.icon("b0", HICOLOR, "48", "x.png");
    // A directory named like an icon is not listed at all.
    std::fs::create_dir(f.base("b0").join("t/48/dir.png")).unwrap();
    // Only broken links: the unthemed icon is the first that exists.
    symlink(
        f.root.join("nowhere.png"),
        f.base("b0").join("t/48/gone.png"),
    )
    .unwrap();
    let loose = f.icon("b1", "", "", "gone.png");

    let index = f.load("t");
    let request = |name| Request::new(name, 48);
    assert_eq!(index.find(&request("alias")).unwrap().path, alias);
    // The pure lookup reads no file, so it answers with the broken link;
    // `find` checks, and goes on.
    assert_eq!(index.lookup(&request("x")).unwrap().path, broken);
    assert_eq!(index.find(&request("x")).unwrap().path, other_size);
    let all: Vec<_> = index
        .candidates(&request("x"))
        .unwrap()
        .map(|i| i.path)
        .collect();
    assert_eq!(
        all,
        [
            broken.clone(),
            f.base("b0").join("t/48/x.svg"),
            other_size.clone(),
            f.base("b0").join("hicolor/48/x.png"),
        ]
    );
    assert_eq!(index.find(&request("gone")).unwrap().path, loose);
    assert_eq!(found(&index, "dir"), None);
}

#[test]
fn answers_an_absolute_path_as_itself_when_it_exists() {
    let f = Fixture::new();
    let png = f.icon("b0", "", "", "app.png");
    write(&f.root.join("app.jpg"), "jpeg");
    let index = f.load(HICOLOR);
    let by_path = |path: &std::path::Path| index.find(&Request::new(path.to_str().unwrap(), 48));
    let icon = by_path(&png).unwrap();
    assert_eq!((icon.path, icon.format), (png.clone(), Format::Png));
    let gone = f.root.join("gone.svg");
    assert_eq!(by_path(&gone), Err(LookupError::NoFile(gone)));
    // Another format is answered too; drawing it is the renderer's
    // business.
    let jpeg = f.root.join("app.jpg");
    let icon = by_path(&jpeg).unwrap();
    assert_eq!((icon.path, icon.format), (jpeg, Format::Other));
    // `find` answers names as `lookup` does.
    assert_eq!(index.find(&Request::new("app", 48)).unwrap().path, png);
}

#[test]
fn reports_an_invalid_theme_name_from_inherits() {
    let f = Fixture::new();
    f.theme("b0", "t", &theme_text("../escape", &["48:48:Fixed"]));
    let index = f.load("t");
    assert!(
        index
            .problems()
            .iter()
            .any(|p| p.theme == "../escape" && p.kind == ProblemKind::InvalidName)
    );
}

#[test]
fn builds_the_search_path_from_the_base_directories() {
    let dirs = BaseDirs::from_vars(|name| match name {
        "HOME" => Some("/home/u".into()),
        "XDG_DATA_DIRS" => Some("/usr/local/share:/usr/share".into()),
        _ => None,
    });
    let search_path = SearchPath::new(&dirs);
    let dirs: Vec<String> = search_path
        .dirs()
        .iter()
        .map(|d| d.display().to_string())
        .collect();
    assert_eq!(
        dirs,
        [
            "/home/u/.icons",
            "/home/u/.local/share/icons",
            "/usr/local/share/icons",
            "/usr/share/icons",
            "/usr/share/pixmaps",
        ]
    );
}
