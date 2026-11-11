//! What a session's compositor renders with: the software rasteriser, or
//! the machine's GPU.
//!
//! sway and labwc render on wlroots' pixman renderer by default here, which
//! offers no dmabuf, so a client presenting through Vulkan (rinch's GPU
//! renderer, on wgpu) has nothing to present on. On the GPU they render
//! with wlroots' GLES2 renderer on the GPU's render node
//! (`WLR_RENDER_DRM_DEVICE`): a render node only, which needs no KMS, no
//! seat and no logind. KWin composites with OpenGL either way, on the DRM
//! device its virtual backend finds; the choice changes nothing for it.

use std::path::PathBuf;
use std::process::Command;

use super::{Compositor, RenderNodes};
use crate::error::Error;

/// What the compositor renders with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum CompositorRenderer {
    /// The software rasteriser (wlroots' pixman renderer).
    #[default]
    Software,
    /// The machine's GPU (wlroots' GLES2 renderer on the GPU's render node);
    /// fails where the machine has no GPU.
    Gpu,
}

/// How a wlroots compositor renders, once it is decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wlroots {
    /// `WLR_RENDERER=pixman`.
    Pixman,
    /// `WLR_RENDERER=gles2` on this render node.
    Gles2(PathBuf),
}

impl CompositorRenderer {
    /// What `compositor` renders with for a client that presents with
    /// rinch's GPU renderer (`gpu_client`) or its software one: the GPU for
    /// a GPU client under sway or labwc, the only session it can present
    /// on; the software rasteriser otherwise, as every other session has.
    pub(crate) fn for_client(compositor: Compositor, gpu_client: bool) -> Self {
        match compositor {
            Compositor::Sway | Compositor::Labwc if gpu_client => Self::Gpu,
            Compositor::Kwin | Compositor::Sway | Compositor::Labwc => Self::Software,
        }
    }

    /// How a wlroots compositor renders for this choice on a machine with
    /// `nodes`.
    pub(crate) fn wlroots(self, nodes: &RenderNodes) -> Result<Wlroots, Error> {
        match self {
            Self::Software => Ok(Wlroots::Pixman),
            Self::Gpu => nodes.gpu().map(Wlroots::Gles2).ok_or_else(|| Error::NoGpu {
                nodes: nodes.to_string(),
            }),
        }
    }
}

impl Wlroots {
    /// Sets `command`'s wlroots renderer variables.
    pub(crate) fn apply(&self, command: &mut Command) {
        match self {
            Self::Pixman => {
                command.env("WLR_RENDERER", "pixman");
            }
            Self::Gles2(node) => {
                command
                    .env("WLR_RENDERER", "gles2")
                    .env("WLR_RENDER_DRM_DEVICE", node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn env(command: &Command, name: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(name))
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn the_gpu_renders_on_its_node_and_needs_one() {
        let gpu = RenderNodes {
            nodes: vec![("renderD128".into(), Some("amdgpu".into()))],
        };
        let mut command = Command::new("sway");
        CompositorRenderer::Gpu
            .wlroots(&gpu)
            .unwrap()
            .apply(&mut command);
        assert_eq!(env(&command, "WLR_RENDERER").as_deref(), Some("gles2"));
        assert_eq!(
            env(&command, "WLR_RENDER_DRM_DEVICE").as_deref(),
            Some("/dev/dri/renderD128")
        );

        let vgem = RenderNodes {
            nodes: vec![("renderD128".into(), Some("vgem".into()))],
        };
        assert!(CompositorRenderer::Gpu.wlroots(&vgem).is_err());
        let mut command = Command::new("labwc");
        CompositorRenderer::Software
            .wlroots(&vgem)
            .unwrap()
            .apply(&mut command);
        assert_eq!(env(&command, "WLR_RENDERER").as_deref(), Some("pixman"));
        assert_eq!(env(&command, "WLR_RENDER_DRM_DEVICE"), None);
    }
}
