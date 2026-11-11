//! What the measured program's own output says: its frames, and the GPU
//! adapter wgpu chose.
//!
//! - rinch with `RINCH_PERF` set writes `[PERF] frame N: ...` to standard
//!   error after each presented frame, so the first such line is the first
//!   frame on screen, and their count is the frames drawn.
//! - wgpu logs the adapter it settled on at `debug` level, target
//!   `wgpu_core::instance` (`Request adapter result AdapterInfo { name:
//!   ".." .. driver: ".." .. device_type: Cpu .. }`), which rinch's log
//!   subscriber prints to standard output when `RUST_LOG` lets it through.

/// The `RUST_LOG` directive that makes wgpu name the adapter it chose.
pub(crate) const ADAPTER_DIRECTIVE: &str = "wgpu_core::instance=debug";

/// The `RUST_LOG` for the measured program: the caller's, if any, with the
/// adapter directive added; otherwise warnings and the directive.
pub(crate) fn rust_log(callers: Option<&str>) -> String {
    match callers {
        Some(filter) if !filter.trim().is_empty() => format!("{filter},{ADAPTER_DIRECTIVE}"),
        _ => format!("warn,{ADAPTER_DIRECTIVE}"),
    }
}

/// The frame number of a `RINCH_PERF` line.
pub(crate) fn frame_number(line: &str) -> Option<u64> {
    let rest = line.strip_prefix("[PERF] frame ")?;
    rest.split(':').next()?.trim().parse().ok()
}

/// The adapter wgpu chose, as it logged it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Adapter {
    /// `name`: the device (`AMD Radeon 890M Graphics (RADV STRIX1)`).
    pub(crate) name: String,
    /// `driver`: `radv`, `llvmpipe`, ...
    pub(crate) driver: String,
    /// `driver_info`: the driver's version (`Mesa 26.0.8-1ubuntu0.3`).
    pub(crate) driver_info: String,
    /// `device_type`: `IntegratedGpu`, `DiscreteGpu`, `Cpu`, ...
    pub(crate) device_type: String,
    /// `backend`: `Vulkan`, ...
    pub(crate) backend: String,
}

impl Adapter {
    /// Whether this is a software rasteriser (lavapipe, SwiftShader): wgpu's
    /// `DeviceType::Cpu`.
    pub(crate) fn is_software(&self) -> bool {
        self.device_type == "Cpu"
    }
}

/// The adapter in a log line, if the line is wgpu's "Request adapter
/// result". ANSI colour codes in the line are ignored.
pub(crate) fn adapter(line: &str) -> Option<Adapter> {
    let line = strip_ansi(line);
    let info = &line[line.find("Request adapter result AdapterInfo {")?..];
    Some(Adapter {
        name: quoted_field(info, "name")?,
        driver: quoted_field(info, "driver")?,
        driver_info: quoted_field(info, "driver_info").unwrap_or_default(),
        device_type: bare_field(info, "device_type")?,
        backend: bare_field(info, "backend")?,
    })
}

/// `field: "value"` in a `Debug` struct.
fn quoted_field(info: &str, field: &str) -> Option<String> {
    let start = info.find(&format!(" {field}: \""))? + field.len() + 4;
    let end = start + info[start..].find('"')?;
    Some(info[start..end].to_owned())
}

/// `field: Value,` in a `Debug` struct.
fn bare_field(info: &str, field: &str) -> Option<String> {
    let start = info.find(&format!(" {field}: "))? + field.len() + 3;
    let value: String = info[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!value.is_empty()).then_some(value)
}

/// `line` without ANSI escape sequences (`ESC [ ... letter`).
pub(crate) fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHOSEN: &str = "2026-11-10T22:06:45.811352Z \u{1b}[34mDEBUG\u{1b}[0m \
        wgpu_core::instance: Request adapter result AdapterInfo { name: \"llvmpipe (LLVM \
        21.1.8, 256 bits)\", vendor: 65541, device: 0, device_type: Cpu, device_pci_bus_id: \
        \"\", driver: \"llvmpipe\", driver_info: \"Mesa 26.0.8-1ubuntu0.3 (LLVM 21.1.8)\", \
        backend: Vulkan, subgroup_min_size: 8, subgroup_max_size: 8, transient_saves_memory: \
        Some(false), limit_bucket: None }";

    #[test]
    fn reads_the_chosen_adapter() {
        let adapter = adapter(CHOSEN).expect("an adapter");
        assert_eq!(adapter.name, "llvmpipe (LLVM 21.1.8, 256 bits)");
        assert_eq!(adapter.driver, "llvmpipe");
        assert_eq!(adapter.driver_info, "Mesa 26.0.8-1ubuntu0.3 (LLVM 21.1.8)");
        assert_eq!(adapter.device_type, "Cpu");
        assert_eq!(adapter.backend, "Vulkan");
        assert!(adapter.is_software());
    }

    #[test]
    fn other_adapter_lines_are_not_the_choice() {
        let rejected = "DEBUG wgpu_core::instance: Adapter AdapterInfo { name: \"AMD Radeon \
            890M Graphics (RADV STRIX1)\", device_type: IntegratedGpu, driver: \"radv\", \
            backend: Vulkan } not compatible with surface";
        assert_eq!(adapter(rejected), None);
        assert_eq!(
            adapter("INFO rinch: presenting with the software renderer"),
            None
        );
    }

    #[test]
    fn frame_lines() {
        assert_eq!(
            frame_number("[PERF] frame 1: style 0.29ms layout 31.73ms"),
            Some(1)
        );
        assert_eq!(frame_number("[PERF] frame 12: | paint_frames=1"), Some(12));
        assert_eq!(frame_number("deskrinch-panel: clock: cannot start"), None);
        assert_eq!(frame_number("[PERF] frame x:"), None);
    }

    #[test]
    fn rust_log_keeps_the_callers_filter() {
        assert_eq!(rust_log(None), "warn,wgpu_core::instance=debug");
        assert_eq!(rust_log(Some(" ")), "warn,wgpu_core::instance=debug");
        assert_eq!(
            rust_log(Some("info,rinch=debug")),
            "info,rinch=debug,wgpu_core::instance=debug"
        );
    }

    #[test]
    fn ansi_codes_are_removed() {
        assert_eq!(
            strip_ansi("\u{1b}[2m2026\u{1b}[0m \u{1b}[34mDEBUG"),
            "2026 DEBUG"
        );
    }
}
