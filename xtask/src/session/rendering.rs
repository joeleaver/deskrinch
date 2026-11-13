//! What a session's compositor renders with: the software rasteriser, or
//! the machine's GPU.
//!
//! sway and labwc render on wlroots' pixman renderer by default here, which
//! offers no dmabuf, so a client presenting through Vulkan (rinch's GPU
//! renderer, on wgpu) has nothing to present on. On the GPU they render
//! with wlroots' GLES2 renderer on the GPU's render node
//! (`WLR_RENDER_DRM_DEVICE`): a render node only, which needs no KMS, no
//! seat and no logind. A headless KWin composites with OpenGL either way,
//! on the DRM device its virtual backend finds; the choice changes nothing
//! for it. A KWin in a window composites with OpenGL on the GPU, and with
//! `QPainter` (`KWIN_COMPOSE=Q`) in software.

use std::path::PathBuf;
use std::process::Command;

use super::{Compositor, Display, RenderNodes, Session};
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
    /// Sets `command`'s wlroots renderer variables. On the GPU, the
    /// compositor also logs at `info` (`--verbose`, which sway and labwc
    /// both take), where wlroots says which renderer it created and on what.
    pub(crate) fn apply(&self, command: &mut Command) {
        match self {
            Self::Pixman => {
                command.env("WLR_RENDERER", "pixman");
            }
            Self::Gles2(node) => {
                command
                    .env("WLR_RENDERER", "gles2")
                    .env("WLR_RENDER_DRM_DEVICE", node)
                    .arg("--verbose");
            }
        }
    }
}

/// wlroots' line when it has created its GLES2 renderer.
const CREATED_GLES2: &str = "Creating GLES2 renderer";
/// wlroots' line naming the GL implementation it renders with.
const GL_RENDERER: &str = "GL renderer: ";
/// Mesa's software rasterisers, which a GL renderer line can name: GLES2 on
/// one of them is no GPU.
const SOFTWARE_GL: &[&str] = &["llvmpipe", "softpipe"];

/// Why a wlroots compositor asked to render on the GPU is not doing so.
const NOT_ON_THE_GPU: &str = "the compositor was to render on the GPU, but it does not: \
     a client presenting with rinch's GPU renderer would test something else";

/// What `log`, a wlroots compositor's `--verbose` log, says about its
/// renderer: the GL renderer it names when it created its GLES2 renderer
/// on a GPU, or the line that says otherwise (`(none)` when it said
/// nothing).
fn gpu_renderer(log: &str) -> Result<&str, String> {
    if !log.lines().any(|line| line.contains(CREATED_GLES2)) {
        return Err(format!("no \"{CREATED_GLES2}\" line"));
    }
    let named = log
        .lines()
        .find_map(|line| line.split_once(GL_RENDERER).map(|(_, named)| named.trim()));
    match named {
        Some(named) if !SOFTWARE_GL.iter().any(|software| named.contains(software)) => Ok(named),
        Some(named) => Err(format!("{GL_RENDERER}{named}")),
        None => Err(format!("{GL_RENDERER}(none)")),
    }
}

/// KWin's lines when it renders nothing into its output: in a window on a
/// machine where its Wayland backend finds no device it can allocate
/// buffers on (a GPU-less runner's vgem), it says so for every frame.
const CANNOT_PRESENT: &[&str] = &[
    "Rendering a layer failed",
    "Failed to find a working output layer configuration",
];

/// Why a KWin that cannot present is no use.
const PRESENTS_NOTHING: &str = "KWin renders nothing into its window: it found no buffers \
     to render into; with `--compositor-renderer software` it composites with QPainter";

/// The first line of KWin's `log` that says it cannot present, if any.
fn cannot_present(log: &str) -> Option<&str> {
    log.lines()
        .find(|line| CANNOT_PRESENT.iter().any(|said| line.contains(said)))
}

impl Session {
    /// Fails the session if KWin, its outputs in windows, has said it
    /// cannot present: a window that would stay empty.
    pub(crate) fn check_presents(&self) -> Result<(), Error> {
        if self.compositor != Compositor::Kwin || self.display == Display::Headless {
            return Ok(());
        }
        let log = self.log();
        match cannot_present(&log) {
            None => Ok(()),
            Some(line) => Err(Error::CompositorUnfit {
                compositor: self.compositor.name(),
                why: PRESENTS_NOTHING,
                line: line.to_owned(),
                log: log.clone(),
            }),
        }
    }

    /// Fails the session unless a wlroots compositor asked to render on the
    /// GPU says it does: it created its GLES2 renderer, on a GL that is not
    /// Mesa's software rasteriser.
    pub(super) fn check_wlroots_renderer(&self) -> Result<(), Error> {
        if self.renderer != CompositorRenderer::Gpu || self.compositor == Compositor::Kwin {
            return Ok(());
        }
        let log = self.log();
        match gpu_renderer(&log) {
            Ok(_) => Ok(()),
            Err(line) => Err(Error::CompositorUnfit {
                compositor: self.compositor.name(),
                why: NOT_ON_THE_GPU,
                line,
                log,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn kwin_saying_it_cannot_present_is_found() {
        // As a nested KWin 6.6 logged it on a GPU-less CI runner.
        let log = "Accepting client connections on sockets: QList(\"wayland-0\")\n\
                   kwin_wayland_backend: Could not find a suitable render format\n\
                   kwin_core: Rendering a layer failed!\n";
        assert_eq!(
            cannot_present(log),
            Some("kwin_core: Rendering a layer failed!")
        );
        assert_eq!(cannot_present("Accepting client connections\n"), None);
    }

    fn env(command: &Command, name: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(name))
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    /// As sway 1.11 and labwc 0.9.3 (wlroots 0.19) log it with `--verbose`.
    const ON_THE_GPU: &str = "\
00:00:00.025 [INFO] [wlr] [render/wlr_renderer.c:157] Opening DRM render node '/dev/dri/renderD128' from WLR_RENDER_DRM_DEVICE
00:00:00.233 [INFO] [wlr] [render/gles2/renderer.c:538] Creating GLES2 renderer
00:00:00.233 [INFO] [wlr] [render/gles2/renderer.c:541] GL renderer: AMD Radeon 890M Graphics (radeonsi, strix1, ACO, DRM 3.64, 7.0.0-42-generic)
";

    #[test]
    fn reads_what_wlroots_renders_with() {
        assert_eq!(
            gpu_renderer(ON_THE_GPU),
            Ok("AMD Radeon 890M Graphics (radeonsi, strix1, ACO, DRM 3.64, 7.0.0-42-generic)")
        );
        let llvmpipe = ON_THE_GPU.replace(
            "AMD Radeon 890M Graphics (radeonsi, strix1, ACO, DRM 3.64, 7.0.0-42-generic)",
            "llvmpipe (LLVM 20.1.8, 256 bits)",
        );
        assert_eq!(
            gpu_renderer(&llvmpipe),
            Err("GL renderer: llvmpipe (LLVM 20.1.8, 256 bits)".to_owned())
        );
        let pixman =
            "00:00:00.020 [INFO] [wlr] [render/pixman/renderer.c:306] Creating pixman renderer\n";
        assert_eq!(
            gpu_renderer(pixman),
            Err("no \"Creating GLES2 renderer\" line".to_owned())
        );
        let unnamed = "[render/gles2/renderer.c:538] Creating GLES2 renderer\n";
        assert_eq!(gpu_renderer(unnamed), Err("GL renderer: (none)".to_owned()));
    }

    #[test]
    fn a_gpu_client_gets_a_gpu_session_only_where_it_needs_one() {
        assert_eq!(
            CompositorRenderer::for_client(Compositor::Sway, true),
            CompositorRenderer::Gpu
        );
        assert_eq!(
            CompositorRenderer::for_client(Compositor::Labwc, true),
            CompositorRenderer::Gpu
        );
        assert_eq!(
            CompositorRenderer::for_client(Compositor::Kwin, true),
            CompositorRenderer::Software
        );
        assert_eq!(
            CompositorRenderer::for_client(Compositor::Sway, false),
            CompositorRenderer::Software
        );
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
