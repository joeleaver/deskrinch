//! Which of rinch's renderers a run tests the suites that render with.
//!
//! The software renderer always. The GPU renderer where the machine seems to
//! have a GPU, and only under KWin: the sessions run sway and labwc with
//! `WLR_RENDERER=pixman`, which offers no dmabuf, so rinch's GPU renderer
//! cannot present there.

use std::fmt;
use std::fs;
use std::path::Path;

use deskrinch_test_component::Renderer;

use crate::session::Compositor;

/// Where the kernel lists DRM devices.
const SYS_DRM: &str = "/sys/class/drm";

/// Drivers whose render nodes are no GPU: vgem is a virtual device that
/// gives a GPU-less KWin a DRM device to render on with Mesa's software
/// driver.
const NOT_A_GPU: &[&str] = &["vgem"];

/// The renderers `cargo xtask compositor-tests --renderer` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Choice {
    /// rinch's software renderer only.
    Software,
    /// rinch's GPU renderer only, under KWin; fails where the machine has no
    /// GPU.
    Gpu,
    /// The software renderer, and the GPU renderer under KWin where the
    /// machine has a GPU.
    All,
}

/// The machine's render nodes, and which of them are a GPU's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RenderNodes {
    /// Each render node, with its kernel driver (`None` when it names none).
    pub(crate) nodes: Vec<(String, Option<String>)>,
}

impl RenderNodes {
    /// The render nodes under `sys_drm` (`/sys/class/drm`).
    pub(crate) fn read(sys_drm: &Path) -> Self {
        let mut nodes: Vec<(String, Option<String>)> = fs::read_dir(sys_drm)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("renderD"))
            .map(|name| {
                let driver = fs::read_link(sys_drm.join(&name).join("device/driver"))
                    .ok()
                    .and_then(|target| target.file_name()?.to_str().map(ToOwned::to_owned));
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
        self.nodes.iter().any(|(_, driver)| {
            driver
                .as_deref()
                .is_some_and(|driver| !NOT_A_GPU.contains(&driver))
        })
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

/// The renderers a run tests: decided once, and said once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    software: bool,
    gpu: bool,
}

impl Plan {
    /// The renderers `choice` asks for on a machine with `nodes`, and the
    /// line that says so; an error when the GPU is asked for and the machine
    /// has none.
    pub(crate) fn new(choice: Choice, nodes: &RenderNodes) -> Result<(Self, String), String> {
        let gpu = nodes.has_gpu();
        match (choice, gpu) {
            (Choice::Software, _) => Ok((
                Self {
                    software: true,
                    gpu: false,
                },
                "renderers: software (as asked)".to_owned(),
            )),
            (Choice::Gpu, false) => Err(format!(
                "--renderer gpu, but this machine has no GPU to present on: {nodes}"
            )),
            (Choice::Gpu, true) => Ok((
                Self {
                    software: false,
                    gpu: true,
                },
                format!("renderers: gpu under KWin only (as asked; {nodes})"),
            )),
            (Choice::All, true) => Ok((
                Self {
                    software: true,
                    gpu: true,
                },
                format!("renderers: software, and gpu under KWin ({nodes})"),
            )),
            (Choice::All, false) => Ok((
                Self {
                    software: true,
                    gpu: false,
                },
                format!("renderers: software only; the GPU renderer is left out: {nodes}"),
            )),
        }
    }

    /// The machine's plan for `choice`, from its render nodes.
    pub(crate) fn for_machine(choice: Choice) -> Result<(Self, String), String> {
        Self::new(choice, &RenderNodes::read(Path::new(SYS_DRM)))
    }

    /// The renderers a rendering suite runs with under `compositor`.
    pub(crate) fn under(&self, compositor: Compositor) -> Vec<Renderer> {
        let mut renderers = Vec::new();
        if self.software {
            renderers.push(Renderer::Software);
        }
        if self.gpu && compositor == Compositor::Kwin {
            renderers.push(Renderer::Gpu);
        }
        renderers
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
    fn no_render_node_is_no_gpu() {
        let dir = sys_drm(&[]);
        let nodes = RenderNodes::read(dir.path());
        assert!(!nodes.has_gpu());
        assert_eq!(nodes.to_string(), "no render node");
        let unbound = sys_drm(&[("renderD128", None)]);
        assert!(!RenderNodes::read(unbound.path()).has_gpu());
    }

    #[test]
    fn plans_say_what_they_leave_out() {
        let none = RenderNodes::default();
        let (plan, said) = Plan::new(Choice::All, &none).unwrap();
        assert_eq!(plan.under(Compositor::Kwin), [Renderer::Software]);
        assert!(said.contains("left out: no render node"), "{said}");
        assert!(Plan::new(Choice::Gpu, &none).is_err());

        let gpu = RenderNodes {
            nodes: vec![("renderD128".into(), Some("amdgpu".into()))],
        };
        let (plan, _) = Plan::new(Choice::All, &gpu).unwrap();
        assert_eq!(
            plan.under(Compositor::Kwin),
            [Renderer::Software, Renderer::Gpu]
        );
        assert_eq!(plan.under(Compositor::Sway), [Renderer::Software]);
        let (plan, _) = Plan::new(Choice::Gpu, &gpu).unwrap();
        assert_eq!(plan.under(Compositor::Labwc), []);
        let (plan, _) = Plan::new(Choice::Software, &gpu).unwrap();
        assert_eq!(plan.under(Compositor::Kwin), [Renderer::Software]);
    }
}
