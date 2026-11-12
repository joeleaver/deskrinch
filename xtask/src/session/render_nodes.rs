//! The machine's DRM render nodes, and the guess at which is a GPU's: the
//! one fact `cargo xtask compositor-tests` plans its renderers from, and a
//! session renders on when it is asked to render with the GPU.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the kernel lists DRM devices.
const SYS_DRM: &str = "/sys/class/drm";
/// Where their device nodes are.
const DEV_DRI: &str = "/dev/dri";

/// Drivers whose render nodes are no GPU: vgem is a virtual device that
/// gives a GPU-less KWin a DRM device to render on with Mesa's software
/// driver.
const NOT_A_GPU: &[&str] = &["vgem"];

/// The machine's render nodes, and which of them are a GPU's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RenderNodes {
    /// Each render node, with its kernel driver (`None` when it names none),
    /// or for a virtual device of [`NOT_A_GPU`], that device's name.
    pub(crate) nodes: Vec<(String, Option<String>)>,
}

impl RenderNodes {
    /// The machine's render nodes.
    pub(crate) fn of_machine() -> Self {
        Self::read(Path::new(SYS_DRM))
    }

    /// The render nodes under `sys_drm` (`/sys/class/drm`).
    pub(crate) fn read(sys_drm: &Path) -> Self {
        let mut nodes: Vec<(String, Option<String>)> = fs::read_dir(sys_drm)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("renderD"))
            .map(|name| {
                let device = sys_drm.join(&name).join("device");
                let base = |link: &Path| {
                    fs::read_link(link)
                        .ok()
                        .and_then(|target| target.file_name()?.to_str().map(ToOwned::to_owned))
                };
                // A virtual device by its own name: since Linux 6.15 vgem
                // is a faux device (`/sys/devices/faux/vgem`), bound to the
                // generic `faux_driver`, which names no GPU either way.
                let driver = base(&device)
                    .filter(|own| NOT_A_GPU.contains(&own.as_str()))
                    .or_else(|| base(&device.join("driver")));
                (name, driver)
            })
            .collect();
        nodes.sort();
        Self { nodes }
    }

    /// Whether the machine seems to have a GPU: a render node whose driver
    /// is not [`NOT_A_GPU`]'s. A guess about what wgpu will accept; the
    /// tests' own assertion of which renderer presented is the check.
    pub(crate) fn has_gpu(&self) -> bool {
        self.gpu().is_some()
    }

    /// The device of the first GPU's render node (`/dev/dri/renderD128`),
    /// by the same guess as [`has_gpu`](Self::has_gpu).
    pub(crate) fn gpu(&self) -> Option<PathBuf> {
        self.nodes
            .iter()
            .find(|(_, driver)| {
                driver
                    .as_deref()
                    .is_some_and(|driver| !NOT_A_GPU.contains(&driver))
            })
            .map(|(node, _)| Path::new(DEV_DRI).join(node))
    }
}

impl fmt::Display for RenderNodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.nodes.is_empty() {
            return f.write_str("no render node");
        }
        let listed: Vec<String> = self
            .nodes
            .iter()
            .map(|(node, driver)| match driver {
                Some(driver) if NOT_A_GPU.contains(&driver.as_str()) => {
                    format!("{node} ({driver}, ignored: not a GPU)")
                }
                Some(driver) => format!("{node} ({driver})"),
                None => format!("{node} (no driver)"),
            })
            .collect();
        f.write_str(&listed.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// A `/sys/class/drm` with `nodes`, each bound to its driver (or none).
    fn sys_drm(nodes: &[(&str, Option<&str>)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let drivers = dir.path().join("drivers");
        for (node, driver) in nodes {
            let device = dir.path().join(node).join("device");
            fs::create_dir_all(&device).unwrap();
            if let Some(driver) = driver {
                fs::create_dir_all(drivers.join(driver)).unwrap();
                symlink(drivers.join(driver), device.join("driver")).unwrap();
            }
        }
        // A card node is not a render node.
        fs::create_dir_all(dir.path().join("card0/device")).unwrap();
        dir
    }

    #[test]
    fn a_real_driver_is_a_gpu() {
        let dir = sys_drm(&[("renderD128", Some("amdgpu"))]);
        let nodes = RenderNodes::read(dir.path());
        assert!(nodes.has_gpu());
        assert_eq!(nodes.to_string(), "renderD128 (amdgpu)");
    }

    #[test]
    fn vgem_alone_is_no_gpu() {
        let dir = sys_drm(&[("renderD128", Some("vgem"))]);
        let nodes = RenderNodes::read(dir.path());
        assert!(!nodes.has_gpu());
        assert_eq!(nodes.to_string(), "renderD128 (vgem, ignored: not a GPU)");
        let both = sys_drm(&[("renderD128", Some("vgem")), ("renderD129", Some("i915"))]);
        assert!(RenderNodes::read(both.path()).has_gpu());
    }

    #[test]
    fn vgem_as_a_faux_device_is_no_gpu() {
        // As Linux 6.15 and later list it: the device is
        // `/sys/devices/faux/vgem`, its driver the generic `faux_driver`.
        let dir = tempfile::tempdir().unwrap();
        let devices = dir.path().join("devices/faux");
        let vgem = devices.join("vgem");
        let driver = dir.path().join("drivers/faux_driver");
        fs::create_dir_all(&vgem).unwrap();
        fs::create_dir_all(&driver).unwrap();
        symlink(&driver, vgem.join("driver")).unwrap();
        fs::create_dir_all(dir.path().join("renderD128")).unwrap();
        symlink(&vgem, dir.path().join("renderD128/device")).unwrap();
        let nodes = RenderNodes::read(dir.path());
        assert!(!nodes.has_gpu());
        assert_eq!(nodes.to_string(), "renderD128 (vgem, ignored: not a GPU)");
    }

    #[test]
    fn no_render_node_is_no_gpu() {
        let dir = sys_drm(&[]);
        let nodes = RenderNodes::read(dir.path());
        assert!(!nodes.has_gpu());
        assert_eq!(nodes.to_string(), "no render node");
        let unbound = sys_drm(&[("renderD128", None)]);
        assert!(!RenderNodes::read(unbound.path()).has_gpu());
    }

    #[test]
    fn the_gpu_is_the_first_real_drivers_node() {
        let dir = sys_drm(&[("renderD128", Some("vgem")), ("renderD129", Some("i915"))]);
        assert_eq!(
            RenderNodes::read(dir.path()).gpu(),
            Some(PathBuf::from("/dev/dri/renderD129"))
        );
        let none = sys_drm(&[("renderD128", Some("vgem"))]);
        assert_eq!(RenderNodes::read(none.path()).gpu(), None);
    }
}
