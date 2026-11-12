//! Which of rinch's renderers a run tests the suites that render with.
//!
//! The software renderer always. The GPU renderer where the machine seems to
//! have a GPU, under every compositor: under sway and labwc in a session
//! whose compositor renders on the GPU too (wlroots' pixman renderer offers
//! no dmabuf, so rinch's GPU renderer cannot present on it).

use deskrinch_test_component::Renderer;

use crate::session::RenderNodes;

/// The renderers `cargo xtask compositor-tests --renderer` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Choice {
    /// rinch's software renderer only.
    Software,
    /// rinch's GPU renderer only; fails where the machine has no GPU.
    Gpu,
    /// The software renderer, and the GPU renderer where the machine has a
    /// GPU.
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
                format!("renderers: gpu only (as asked; {nodes})"),
            )),
            (Choice::All, true) => Ok((
                Self {
                    software: true,
                    gpu: true,
                },
                format!("renderers: software, and gpu ({nodes})"),
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

    /// The renderers a rendering suite runs with.
    pub(crate) fn renderers(&self) -> Vec<Renderer> {
        let mut renderers = Vec::new();
        if self.software {
            renderers.push(Renderer::Software);
        }
        if self.gpu {
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
        assert_eq!(plan.renderers(), [Renderer::Software]);
        assert!(said.contains("left out: no render node"), "{said}");
        assert!(Plan::new(Choice::Gpu, &none).is_err());

        let gpu = RenderNodes {
            nodes: vec![("renderD128".into(), Some("amdgpu".into()))],
        };
        let (plan, said) = Plan::new(Choice::All, &gpu).unwrap();
        assert_eq!(plan.renderers(), [Renderer::Software, Renderer::Gpu]);
        assert_eq!(said, "renderers: software, and gpu (renderD128 (amdgpu))");
        let (plan, _) = Plan::new(Choice::Gpu, &gpu).unwrap();
        assert_eq!(plan.renderers(), [Renderer::Gpu]);
        let (plan, _) = Plan::new(Choice::Software, &gpu).unwrap();
        assert_eq!(plan.renderers(), [Renderer::Software]);
    }
}
