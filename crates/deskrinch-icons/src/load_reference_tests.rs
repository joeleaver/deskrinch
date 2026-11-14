//! The compact index against a reference model of the index it replaced:
//! the same tree listed with `std::fs::read_dir` and `DirEntry::file_type`,
//! as `load` listed it before, into the old shape (a map from each icon
//! name to its sorted locations), and the old lookup order read from it.
//! This is test-only code: the model exists to disagree with the new
//! listing and table if they differ.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use rustix::fs::{CWD, FileType, Mode, mknodat};

use crate::candidates::names;
use crate::index::{Format, Index};
use crate::load::SearchPath;
use crate::lookup::{HICOLOR, Icon, Request};
use crate::theme::Theme;

/// The old shape: each name's files as `(directory, base, format)`, sorted.
type OldIcons = BTreeMap<String, Vec<(usize, usize, Format)>>;

/// The icons directly in `dir`, as the old `Index::list` found them.
fn old_list(dir: &Path) -> Vec<(String, Format)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let (stem, format) = Format::split(file_name.to_str()?)?;
            let file_type = entry.file_type().ok()?;
            let listed = file_type.is_file() || file_type.is_symlink();
            listed.then(|| (stem.to_owned(), format))
        })
        .collect()
}

/// A theme's icons in the old shape, as the old `load_theme` built them.
fn old_theme(bases: &[PathBuf], theme: &Theme) -> OldIcons {
    let mut icons = OldIcons::new();
    for (directory, dir) in theme.directories.iter().enumerate() {
        for (base, root) in bases.iter().enumerate() {
            for (icon, format) in old_list(&root.join(&theme.name).join(&dir.path)) {
                icons
                    .entry(icon)
                    .or_default()
                    .push((directory, base, format));
            }
        }
    }
    for at in icons.values_mut() {
        at.sort();
    }
    icons
}

/// The unthemed icons in the old shape.
fn old_unthemed(bases: &[PathBuf]) -> OldIcons {
    let mut icons = OldIcons::new();
    for (base, root) in bases.iter().enumerate() {
        for (icon, format) in old_list(root) {
            icons.entry(icon).or_default().push((0, base, format));
        }
    }
    for at in icons.values_mut() {
        at.sort();
    }
    icons
}

fn file_name(name: &str, format: Format) -> String {
    format!("{name}.{}", format.extension().unwrap())
}

/// The old `Candidates`, read from the old shape: per theme and name, the
/// files whose directory matches, then the rest by distance (stable), then
/// the unthemed files per name.
fn old_candidates(
    bases: &[PathBuf],
    themes: &[(&Theme, OldIcons)],
    unthemed: &OldIcons,
    request: &Request<'_>,
) -> Vec<Icon> {
    let tried = names(request.name, request.fallback == crate::Fallback::Generic);
    let (size, scale) = (request.size, request.scale);
    let mut found = Vec::new();
    for (theme, icons) in themes {
        for name in &tried {
            let Some(at) = icons.get(name) else { continue };
            let directory = |&(d, _, _): &(usize, usize, Format)| &theme.directories[d];
            let (mut exact, mut other): (Vec<_>, Vec<_>) =
                at.iter().partition(|at| directory(at).matches(size, scale));
            other.sort_by_key(|at| directory(at).distance(size, scale));
            exact.append(&mut other);
            for (d, base, format) in exact {
                let dir = &theme.directories[d].path;
                let path = bases[base]
                    .join(&theme.name)
                    .join(dir)
                    .join(file_name(name, format));
                found.push(Icon { path, format });
            }
        }
    }
    for name in &tried {
        for &(_, base, format) in unthemed.get(name).into_iter().flatten() {
            let path = bases[base].join(file_name(name, format));
            found.push(Icon { path, format });
        }
    }
    found
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

const T: &str = "[Icon Theme]\nName=T\nInherits=p\n\
    Directories=16,16@2,48,scalable,32\nScaledDirectories=16@2\n\
    [16]\nSize=16\nType=Fixed\n[16@2]\nSize=16\nScale=2\nType=Fixed\n\
    [48]\nSize=48\nType=Fixed\n[scalable]\nSize=48\nType=Scalable\nMinSize=1\nMaxSize=256\n\
    [32]\nSize=32\nType=Threshold\n";
const P: &str = "[Icon Theme]\nName=P\nDirectories=22,64\n\
    [22]\nSize=22\nType=Fixed\n[64]\nSize=64\nType=Fixed\n";
const HI: &str = "[Icon Theme]\nName=Hicolor\nDirectories=48,scalable\n\
    [48]\nSize=48\nType=Fixed\n[scalable]\nSize=48\nType=Scalable\n";

/// A tree with the awkward cases, in two base directories.
fn tree(root: &Path) -> Vec<PathBuf> {
    let (b0, b1) = (root.join("b0"), root.join("b1"));
    touch(&b0.join("t/index.theme"));
    fs::write(b0.join("t/index.theme"), T).unwrap();
    touch(&b1.join("p/index.theme"));
    fs::write(b1.join("p/index.theme"), P).unwrap();
    touch(&b1.join("hicolor/index.theme"));
    fs::write(b1.join("hicolor/index.theme"), HI).unwrap();
    // The same name in several sizes, scales, formats and bases; names
    // that are prefixes of others.
    for (base, path) in [
        (&b0, "t/16/folder.png"),
        (&b0, "t/16/folder.svg"),
        (&b0, "t/16/folder.xpm"),
        (&b1, "t/16/folder.png"),
        (&b0, "t/16@2/folder.png"),
        (&b1, "t/48/folder.svg"),
        (&b0, "t/scalable/folder.svg"),
        (&b0, "t/32/folder.png"),
        (&b0, "t/48/folder-open.png"),
        (&b1, "t/scalable/folder-open-symbolic.svg"),
        (&b0, "t/16/fold.png"),
        (&b0, "t/48/a-b-c.png"),
        (&b1, "t/32/a.svg"),
        (&b1, "p/22/folder.png"),
        (&b1, "p/64/folder-new.png"),
        (&b1, "p/64/only-parent.xpm"),
        (&b0, "p/22/only-parent.png"),
        (&b1, "hicolor/48/folder.png"),
        (&b1, "hicolor/scalable/hi.svg"),
        (&b1, "hicolor/48/a-b.png"),
        // Unthemed, in both bases, one also themed.
        (&b0, "loose.png"),
        (&b1, "loose.svg"),
        (&b1, "folder.xpm"),
        (&b0, "pixmap-only.xpm"),
    ] {
        touch(&base.join(path));
    }
    let dir = b0.join("t/48");
    // Links to a file, to a directory, and dangling; in a theme and a base.
    symlink("folder-open.png", dir.join("link-file.png")).unwrap();
    symlink("../16", dir.join("link-dir.svg")).unwrap();
    symlink("nowhere.png", dir.join("dangling.png")).unwrap();
    symlink("loose.png", b0.join("loose-link.svg")).unwrap();
    symlink("absent.png", b1.join("loose-dangling.png")).unwrap();
    // Odd names: a directory named like an icon, a name that is not
    // UTF-8, hidden files, an extension alone, unknown or upper-case
    // extensions, a FIFO.
    fs::create_dir_all(dir.join("a-directory.png")).unwrap();
    touch(&dir.join(OsStr::from_bytes(b"not-utf8-\xff.png")));
    touch(&dir.join(".hidden.png"));
    touch(&b1.join(".hidden-loose.svg"));
    touch(&dir.join(".png"));
    touch(&dir.join("upper.PNG"));
    touch(&dir.join("compressed.svgz"));
    touch(&dir.join("no-extension"));
    mknodat(CWD, dir.join("fifo.png"), FileType::Fifo, Mode::RUSR, 0).unwrap();
    vec![b0, b1]
}

#[test]
fn the_compact_index_answers_as_the_old_shape_for_every_name_and_size() {
    let root = tempfile::tempdir().unwrap();
    let bases = tree(root.path());
    let index = Index::load(&SearchPath::from_dirs(bases.clone()), "t");
    assert_eq!(index.problems(), []);
    assert_eq!(index.consulted(), ["t", "p", HICOLOR]);

    let mut themes = Vec::new();
    let mut every_name = BTreeSet::new();
    for name in index.consulted() {
        let theme = index.installed(name).unwrap();
        let old = old_theme(&bases, theme);
        let table = &index.themes[name.as_str()].icons;
        assert_eq!(
            table.names().collect::<Vec<_>>(),
            old.keys().map(String::as_str).collect::<Vec<_>>(),
            "the names of {name}"
        );
        for (icon, at) in &old {
            let new: Vec<_> = table
                .get(icon)
                .iter()
                .map(|at| (at.directory(), at.base(), at.format()))
                .collect();
            assert_eq!(&new, at, "{icon} in {name}");
        }
        every_name.extend(old.keys().cloned());
        themes.push((theme, old));
    }
    let unthemed = old_unthemed(&bases);
    assert_eq!(
        index.unthemed.names().collect::<Vec<_>>(),
        unthemed.keys().map(String::as_str).collect::<Vec<_>>()
    );
    every_name.extend(unthemed.keys().cloned());
    // The tree's cases are in the old shape as the old listing had them.
    let t48 = &themes[0].1;
    for listed in ["link-file", "link-dir", "dangling", ".hidden"] {
        assert!(t48.contains_key(listed), "{listed}");
    }
    for left_out in [
        "a-directory",
        "",
        "upper",
        "compressed",
        "no-extension",
        "fifo",
    ] {
        assert!(!every_name.contains(left_out), "{left_out:?}");
    }
    assert!(!every_name.iter().any(|name| name.starts_with("not-utf8")));
    assert_eq!(index.files(), {
        let in_themes: usize = themes
            .iter()
            .flat_map(|(_, old)| old.values())
            .map(Vec::len)
            .sum();
        in_themes + unthemed.values().map(Vec::len).sum::<usize>()
    });

    every_name.extend(["fol", "folder-open-x", "missing", "a-b-c-d"].map(String::from));
    let mut asked = 0;
    for name in &every_name {
        for size in [1, 16, 22, 24, 32, 48, 64, 256] {
            for scale in [1, 2] {
                let plain = Request::new(name, size).at_scale(scale);
                for request in [plain, plain.with_generic_fallback()] {
                    let new: Vec<Icon> = index.candidates(&request).unwrap().collect();
                    let old = old_candidates(&bases, &themes, &unthemed, &request);
                    assert_eq!(new, old, "{request:?}");
                    asked += 1;
                }
            }
        }
    }
    assert!(asked > 500, "{asked} requests");
}
