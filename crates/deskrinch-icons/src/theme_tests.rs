//! Reading `index.theme`, and the specification's size rules.

use super::*;

/// The specification's example theme, with the comma its text leaves out
/// between `48x48@2/apps` and `48x48/mimetypes`.
pub(crate) const BIRCH: &str = "[Icon Theme]
Name=Birch
Name[sv]=Björk
Comment=Icon theme with a wooden look
Comment[sv]=Träinspirerat ikontema
Inherits=wood,default
Directories=48x48/apps,48x48@2/apps,48x48/mimetypes,32x32/apps,32x32@2/apps,scalable/apps,scalable/mimetypes

[scalable/apps]
Size=48
Type=Scalable
MinSize=1
MaxSize=256
Context=Applications

[scalable/mimetypes]
Size=48
Type=Scalable
MinSize=1
MaxSize=256
Context=MimeTypes

[32x32/apps]
Size=32
Type=Fixed
Context=Applications

[32x32@2/apps]
Size=32
Scale=2
Type=Fixed
Context=Applications

[48x48/apps]
Size=48
Type=Fixed
Context=Applications

[48x48@2/apps]
Size=48
Scale=2
Type=Fixed
Context=Applications

[48x48/mimetypes]
Size=48
Type=Fixed
Context=MimeTypes
";

fn parse(text: &str) -> (Option<Theme>, Vec<ProblemKind>) {
    let mut problems = Vec::new();
    let theme = Theme::parse("t", text.as_bytes(), &mut problems);
    (theme, problems)
}

fn directory(kind: DirectoryKind, size: u32, scale: u32) -> Directory {
    Directory {
        path: "d".into(),
        size,
        scale,
        kind,
        context: None,
    }
}

#[test]
fn reads_the_specifications_example() {
    let (theme, problems) = parse(BIRCH);
    assert_eq!(problems, []);
    let theme = theme.unwrap();
    assert_eq!(theme.title.unwrap().default, "Birch");
    assert_eq!(theme.inherits, ["wood", "default"]);
    let paths: Vec<&str> = theme.directories.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "48x48/apps",
            "48x48@2/apps",
            "48x48/mimetypes",
            "32x32/apps",
            "32x32@2/apps",
            "scalable/apps",
            "scalable/mimetypes"
        ]
    );
    let scaled = &theme.directories[1];
    assert_eq!((scaled.size, scaled.scale), (48, 2));
    assert_eq!(scaled.kind, DirectoryKind::Fixed);
    let scalable = &theme.directories[5];
    assert_eq!(scalable.kind, DirectoryKind::Scalable { min: 1, max: 256 });
    assert_eq!(scalable.context.as_deref(), Some("Applications"));
    assert!(!theme.hidden);
}

#[test]
fn applies_the_defaults_and_adds_scaled_directories_once() {
    let (theme, problems) = parse(
        "[Icon Theme]\nName=T\nDirectories=a,b\nScaledDirectories=b,c\nHidden=true\n\
         [a]\nSize=16\n[b]\nSize=24\nType=Scalable\n[c]\nSize=24\nScale=2\nType=Threshold\nThreshold=5\n",
    );
    assert_eq!(problems, []);
    let theme = theme.unwrap();
    let kinds: Vec<_> = theme
        .directories
        .iter()
        .map(|d| (d.scale, d.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (1, DirectoryKind::Threshold { threshold: 2 }),
            (1, DirectoryKind::Scalable { min: 24, max: 24 }),
            (2, DirectoryKind::Threshold { threshold: 5 }),
        ]
    );
    assert!(theme.hidden);
    assert_eq!(theme.inherits, Vec::<String>::new());
}

#[test]
fn skips_a_malformed_file_or_one_that_is_no_icon_theme() {
    let (theme, problems) = parse("[Icon Theme]\nName=T\nnot a key\n");
    assert_eq!(theme, None);
    assert!(matches!(problems[..], [ProblemKind::Syntax(_)]));
    let (theme, problems) = parse("[Desktop Entry]\nName=T\n");
    assert_eq!(theme, None);
    assert_eq!(problems, [ProblemKind::NotAnIconTheme]);
    let (theme, _) = parse(&String::from_utf8_lossy(&[0xff, 0xfe, 0x00]));
    assert_eq!(theme, None);
}

#[test]
fn leaves_out_unusable_directories_and_reads_on() {
    let (theme, problems) = parse(
        "[Icon Theme]\nName=T\nDirectories=nogroup,nosize,zero,badscale,odd,ok\n\
         [nosize]\nType=Fixed\n[zero]\nSize=0\n[badscale]\nSize=16\nScale=x\n\
         [odd]\nSize=16\nType=Weird\n[ok]\nSize=48\nType=Fixed\n",
    );
    let theme = theme.unwrap();
    let paths: Vec<&str> = theme.directories.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["odd", "ok"]);
    assert_eq!(
        theme.directories[0].kind,
        DirectoryKind::Threshold { threshold: 2 }
    );
    let kinds: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert_eq!(
        kinds,
        [
            "directory nogroup is listed but has no group",
            "directory nosize has no usable Size",
            "directory zero has no usable Size",
            "key Scale in [badscale] is \"x\", not an integer",
            "directory badscale has no usable Scale",
            "directory odd has an unknown Type \"Weird\", read as Threshold",
        ]
    );
}

#[test]
fn matches_sizes_by_the_specifications_rules() {
    let fixed = directory(DirectoryKind::Fixed, 48, 1);
    assert!(fixed.matches(48, 1));
    assert!(!fixed.matches(47, 1));
    assert!(!fixed.matches(48, 2), "the scale must be the same");
    let scalable = directory(DirectoryKind::Scalable { min: 16, max: 256 }, 48, 1);
    assert!(scalable.matches(16, 1) && scalable.matches(256, 1));
    assert!(!scalable.matches(15, 1) && !scalable.matches(257, 1));
    let threshold = directory(DirectoryKind::Threshold { threshold: 2 }, 32, 1);
    assert!(threshold.matches(30, 1) && threshold.matches(34, 1));
    assert!(!threshold.matches(29, 1) && !threshold.matches(35, 1));
}

#[test]
fn measures_distance_in_device_pixels() {
    let fixed = directory(DirectoryKind::Fixed, 48, 1);
    assert_eq!(fixed.distance(48, 1), 0);
    assert_eq!(fixed.distance(32, 1), 16);
    assert_eq!(fixed.distance(24, 2), 0, "24 at 2 is 48 device pixels");
    let fixed_2x = directory(DirectoryKind::Fixed, 48, 2);
    assert_eq!(fixed_2x.distance(48, 1), 48);
    let scalable = directory(DirectoryKind::Scalable { min: 16, max: 64 }, 48, 1);
    assert_eq!(scalable.distance(8, 1), 8);
    assert_eq!(scalable.distance(40, 1), 0);
    assert_eq!(scalable.distance(100, 1), 36);
    let threshold = directory(DirectoryKind::Threshold { threshold: 2 }, 32, 1);
    assert_eq!(threshold.distance(31, 1), 0);
    assert_eq!(threshold.distance(24, 1), 6);
    assert_eq!(threshold.distance(48, 1), 14);
}
