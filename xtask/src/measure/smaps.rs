//! Where a process's resident memory is: `/proc/<pid>/smaps` grouped by
//! what each mapping is (the GPU driver, LLVM, fonts, the program, the
//! heap, ...).
//!
//! The groups are by the mapped file's name. A shared library that several
//! groups use (`libwayland-client`, `libz`) is counted once, in the first
//! group that names it, so the groups add up to the whole.

use std::collections::BTreeMap;

/// A group of mappings and the patterns of mapping names that belong to it,
/// tried in order; the first match wins.
struct Rule {
    group: &'static str,
    /// Substrings of the mapping's name.
    names: &'static [&'static str],
}

/// The groups, most specific first.
const RULES: &[Rule] = &[
    Rule {
        group: "GPU device memory in host RAM (memfd)",
        names: &["/memfd:allocation fd"],
    },
    Rule {
        group: "Vulkan driver: RADV",
        names: &["libvulkan_radeon"],
    },
    Rule {
        group: "Vulkan driver: lavapipe (CPU)",
        names: &["libvulkan_lvp"],
    },
    Rule {
        group: "other Vulkan drivers and layers",
        names: &["libvulkan_", "libVkLayer"],
    },
    Rule {
        group: "Vulkan loader",
        names: &["libvulkan.so"],
    },
    Rule {
        group: "LLVM and its libraries (linked by Mesa)",
        names: &[
            "libLLVM", "libedit", "libtinfo", "libxml2", "libbsd.", "libmd.",
        ],
    },
    Rule {
        group: "Mesa, DRM and window-system glue",
        names: &[
            "libdrm",
            "libgallium",
            "libelf",
            "libEGL",
            "libGL",
            "libgbm",
            "libxshmfence",
            "libdisplay-info",
            "libxcb-dri",
            "libxcb-present",
            "libxcb-sync",
            "libxcb-randr",
            "libxcb-xfixes",
            "libX11-xcb",
            "libzstd",
            "/dev/dri/",
        ],
    },
    Rule {
        group: "Mesa shader cache",
        names: &["mesa_shader_cache"],
    },
    Rule {
        group: "fonts and fontconfig caches",
        names: &[".ttf", ".otf", ".ttc", ".pcf", "/fonts/", "fontconfig/"],
    },
    Rule {
        group: "GTK and its libraries (muda's menus)",
        names: &[
            "libgtk",
            "libgdk",
            "libatk",
            "libgio",
            "libglib",
            "libgobject",
            "libgmodule",
            "libpango",
            "libcairo",
            "libharfbuzz",
            "libepoxy",
            "libpixman",
            "libfribidi",
            "libthai",
            "libdatrie",
            "libgraphite",
            "libpng",
            "libglycin",
            "liblcms2",
            "libseccomp",
            "libfontconfig",
            "libfreetype",
            "libxdo",
            "libXtst",
            "libXRes",
            "libXi.",
            "libXrandr",
            "libXcursor",
            "libXinerama",
            "libXcomposite",
            "libXdamage",
            "libXfixes",
            "libXext",
            "libXrender",
            "libX11",
            "libXau",
            "libXdmcp",
            "libxcb",
            "libxkbcommon",
            "libwayland",
            "libffi",
            "libpcre",
            "libmount",
            "libblkid",
            "libselinux",
            "libatspi",
            "libdbus",
            "libsystemd",
            "libbrotli",
            "libbz2",
            "libexpat",
            "libz.",
            "libatomic",
            "libstdc++",
            "libgcc_s",
        ],
    },
    Rule {
        group: "glibc, ld.so and locales",
        names: &["libc.so", "libm.so", "ld-linux", "/locale/", "gconv"],
    },
    Rule {
        group: "heap ([heap])",
        names: &["[heap]"],
    },
    Rule {
        group: "stacks",
        names: &["[stack"],
    },
];

/// The group of anonymous mappings: what `malloc` hands out beyond the
/// `[heap]` (thread arenas, large allocations), thread stacks, and buffers
/// a driver maps without a name.
pub(crate) const ANONYMOUS: &str = "anonymous (malloc arenas, thread stacks, driver buffers)";
/// The group of the measured program's own binary.
pub(crate) const PROGRAM: &str = "the program's binary";
/// Everything else.
pub(crate) const OTHER: &str = "other";

/// Memory in one group, in kB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Usage {
    /// Resident.
    pub(crate) rss: u64,
    /// Proportional: shared pages divided among the processes sharing them.
    pub(crate) pss: u64,
    /// Anonymous (not backed by a file).
    pub(crate) anonymous: u64,
    /// Swapped out.
    pub(crate) swap: u64,
}

impl Usage {
    fn add(&mut self, other: Self) {
        self.rss += other.rss;
        self.pss += other.pss;
        self.anonymous += other.anonymous;
        self.swap += other.swap;
    }
}

/// The group a mapping named `name` (empty for an anonymous one) belongs
/// to, for a program whose binary is `program`.
pub(crate) fn classify(name: &str, program: &str) -> &'static str {
    if name.is_empty() {
        return ANONYMOUS;
    }
    if !program.is_empty() && name == program {
        return PROGRAM;
    }
    RULES
        .iter()
        .find(|rule| rule.names.iter().any(|pattern| name.contains(pattern)))
        .map_or(OTHER, |rule| rule.group)
}

/// Groups the mappings of an `smaps` text, for a program whose binary is at
/// `program`.
pub(crate) fn parse(smaps: &str, program: &str) -> BTreeMap<&'static str, Usage> {
    let mut groups: BTreeMap<&'static str, Usage> = BTreeMap::new();
    let mut current: Option<(&'static str, Usage)> = None;
    for line in smaps.lines() {
        if let Some(name) = mapping_name(line) {
            if let Some((group, usage)) = current.take() {
                groups.entry(group).or_default().add(usage);
            }
            current = Some((classify(name, program), Usage::default()));
            continue;
        }
        let Some((_, usage)) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Some(kb) = value.split_whitespace().next().and_then(|v| v.parse().ok()) else {
            continue;
        };
        match key {
            "Rss" => usage.rss = kb,
            "Pss" => usage.pss = kb,
            "Anonymous" => usage.anonymous = kb,
            "Swap" => usage.swap = kb,
            _ => {}
        }
    }
    if let Some((group, usage)) = current {
        groups.entry(group).or_default().add(usage);
    }
    groups
}

/// The name of the mapping a header line starts (`7f..-7f.. r-xp 0 fd:01 42
/// /usr/lib/libfoo.so`), empty for an anonymous mapping; `None` for a line
/// that is not a header.
fn mapping_name(line: &str) -> Option<&str> {
    let mut fields = line.splitn(6, ' ');
    let range = fields.next()?;
    let (start, end) = range.split_once('-')?;
    if start.is_empty()
        || !start.bytes().all(|b| b.is_ascii_hexdigit())
        || !end.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    // Permissions, offset, device and inode, then the name after padding.
    for _ in 0..4 {
        fields.next()?;
    }
    Some(fields.next().unwrap_or("").trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAPS: &str = "\
5f00a0000000-5f00a1000000 r-xp 00000000 fd:01 42                         /opt/bin/panel
Rss:               17272 kB
Pss:               17270 kB
Anonymous:             0 kB
Swap:                  0 kB
7f0000000000-7f0003000000 rw-s 00000000 00:01 7                          /memfd:allocation fd (deleted)
Rss:               49152 kB
Pss:               49152 kB
Anonymous:             0 kB
Swap:                  0 kB
7f1000000000-7f1000100000 rw-p 00000000 00:00 0
Rss:                1024 kB
Pss:                1024 kB
Anonymous:          1024 kB
Swap:                 12 kB
7f2000000000-7f2002000000 r-xp 00000000 fd:01 9                          /usr/lib/x86_64-linux-gnu/libLLVM.so.21.1
Rss:               39396 kB
Pss:               19698 kB
Anonymous:             0 kB
Swap:                  0 kB
VmFlags: rd ex mr mw me
7f3000000000-7f3000010000 rw-p 00000000 00:00 0                          [heap]
Rss:                  64 kB
Pss:                  64 kB
Anonymous:            64 kB
Swap:                  0 kB
";

    #[test]
    fn groups_mappings_by_name() {
        let groups = parse(SMAPS, "/opt/bin/panel");
        assert_eq!(groups[PROGRAM].rss, 17_272);
        assert_eq!(groups["GPU device memory in host RAM (memfd)"].rss, 49_152);
        assert_eq!(
            groups[ANONYMOUS],
            Usage {
                rss: 1024,
                pss: 1024,
                anonymous: 1024,
                swap: 12
            }
        );
        assert_eq!(
            groups["LLVM and its libraries (linked by Mesa)"].pss,
            19_698
        );
        assert_eq!(groups["heap ([heap])"].anonymous, 64);
        let total: u64 = groups.values().map(|usage| usage.rss).sum();
        assert_eq!(total, 17_272 + 49_152 + 1024 + 39_396 + 64);
    }

    #[test]
    fn the_first_matching_rule_wins() {
        assert_eq!(
            classify("/usr/lib/libvulkan_radeon.so", ""),
            "Vulkan driver: RADV"
        );
        assert_eq!(
            classify("/usr/lib/libvulkan.so.1.4.341", ""),
            "Vulkan loader"
        );
        assert_eq!(
            classify("/usr/lib/libvulkan_intel.so", ""),
            "other Vulkan drivers and layers"
        );
        assert_eq!(
            classify("/home/u/.cache/fontconfig/0bd3-le64.cache-9", ""),
            "fonts and fontconfig caches"
        );
        assert_eq!(classify("/usr/lib/libunknown.so", ""), OTHER);
        assert_eq!(classify("", "/opt/bin/panel"), ANONYMOUS);
    }

    #[test]
    fn header_lines_are_told_from_fields() {
        assert_eq!(
            mapping_name("7f00-7f10 r-xp 00000000 fd:01 9      /usr/lib/libz.so.1"),
            Some("/usr/lib/libz.so.1")
        );
        assert_eq!(mapping_name("7f00-7f10 rw-p 00000000 00:00 0 "), Some(""));
        assert_eq!(mapping_name("Rss:               64 kB"), None);
        assert_eq!(mapping_name("VmFlags: rd ex mr mw me"), None);
    }
}
