//! The report's header: what was measured, where and how, and what a
//! reader needs to read its rows (where start-up ended, a program that
//! drew nothing).

use super::environment::environment_set;
use super::renderer::Renderer;
use super::report::Run;
use super::start_end::End;
use super::windows::Expect;
use super::{Request, ShaderCache, system};
use crate::session::{Compositor, CompositorRenderer, RenderNodes};

/// What was measured, where and how.
pub(super) fn header(request: &Request, end: &End) -> String {
    let args: Vec<String> = request
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let header = format!(
        "Measured: {} {}\n\
         Compositor: {} (nested, headless, {} output(s); {})\n\
         Renderer: {}; shader cache: {}; {}\n\
         Machine: {}",
        request.program.to_string_lossy(),
        args.join(" "),
        request.compositor.name(),
        request.outputs,
        match (request.compositor, request.compositor_renderer()) {
            (Compositor::Kwin, _) => "it composites with the GPU through EGL".to_owned(),
            (_, CompositorRenderer::Gpu) => format!(
                "wlroots with the GLES2 renderer on {}, as its log says",
                RenderNodes::of_machine()
                    .gpu()
                    .map_or_else(|| "the GPU".to_owned(), |node| node.display().to_string())
            ),
            (_, CompositorRenderer::Software) => match request.renderer {
                Some(Renderer::Auto) | None => "wlroots with the pixman renderer, which offers no \
                     dmabuf; rinch's GPU renderer would have only a software Vulkan driver, which \
                     its default declines"
                    .to_owned(),
                Some(Renderer::Software | Renderer::Gpu) => {
                    "wlroots with the pixman renderer".to_owned()
                }
            },
        },
        request
            .renderer
            .map_or("as the caller's environment says", Renderer::env),
        match request.shader_cache {
            ShaderCache::Cold => "cold (empty)",
            ShaderCache::Warm => "warm (filled by an unmeasured start)",
        },
        match (&request.summons, request.instructions) {
            (Some(_), true) => "shows, under Callgrind: instruction counts only",
            (Some(_), false) => "shows: CPU from each summon to its first frame",
            (None, true) => "under Callgrind: instruction counts only",
            (None, false) => "settle and idle windows as asked",
        },
        system::describe(),
    );
    let header = match (request.windows, end.is_first_frame()) {
        (Some(n), true) => {
            format!("{header}\nWindows: {n} expected; start-up ends at the last one's first frame")
        }
        (Some(n), false) => format!("{header}\nWindows: {n} expected"),
        (None, _) => header,
    };
    let header = if end.is_first_frame() {
        header
    } else {
        format!(
            "{header}\nStart-up ends at: {} (--start-ends); the figures \"to start-up's end\" run \
             to it, and the settle and the idle window begin after it",
            end.describe(Expect::of(request.windows))
        )
    };
    let header = if request.summons.as_ref().is_some_and(|s| s.prepares) {
        format!(
            "{header}\nPrepares (--prepares): the first show waited for the program's first \
             preparation, then a quiet second"
        )
    } else {
        header
    };
    let header = match request.summons.as_ref().and_then(|s| s.typing.as_ref()) {
        Some(typing) => format!(
            "{header}\nTyped after each show: {:?}, a key at a time, each to settled",
            typing.text
        ),
        None => header,
    };
    let header = match &request.pointer {
        Some(pointer) => format!("{header}\nPointer: {}", pointer.describe()),
        None => header,
    };
    match environment_set(request) {
        Some(set) => format!("{header}\nEnvironment set: {set}"),
        None => header,
    }
}

/// The header's line for runs in none of which the program drew a frame or
/// opened a window, if that is so: its frame rows read 0 because it has no
/// window, not because it failed to draw.
pub(super) fn no_window(runs: &[Run]) -> Option<&'static str> {
    (!runs.is_empty() && runs.iter().all(|run| !run.drew)).then_some(
        "No window: the program presented no frame and opened no window in any run, so its \
         frame rows read 0 by that, not by a failure to draw",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_drew_in_no_run_is_said_to_have_no_window() {
        let drew = |drew| Run {
            drew,
            ..Run::default()
        };
        assert!(no_window(&[drew(false), drew(false)]).is_some());
        assert!(no_window(&[drew(false), drew(true)]).is_none());
        assert!(no_window(&[]).is_none());
    }
}
