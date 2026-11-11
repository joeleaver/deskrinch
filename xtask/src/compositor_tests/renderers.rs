//! Which of rinch's renderers a run tests the suites that render with.
//!
//! The software renderer always. The GPU renderer where the machine seems to
//! have a GPU, and only under KWin: the sessions run sway and labwc with
//! `WLR_RENDERER=pixman`, which offers no dmabuf, so rinch's GPU renderer
//! cannot present there.

use deskrinch_test_component::Renderer;

use crate::session::{Compositor, RenderNodes};

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
        Self::new(choice, &RenderNodes::of_machine())
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
    use super::*;

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
