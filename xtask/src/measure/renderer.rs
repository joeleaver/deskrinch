//! The renderer a measured program is told to use, and the one it must
//! have presented with (`--expect-renderer`).

use super::report;
use crate::Outcome;

/// Which renderer the program is told to use, through rinch's
/// `RINCH_RENDERER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Renderer {
    /// rinch's choice (the GPU, falling back to software).
    Auto,
    /// The GPU renderer (Vello on wgpu).
    Gpu,
    /// The software renderer (tiny-skia).
    Software,
}

impl Renderer {
    pub(super) fn env(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Software => "software",
        }
    }
}

/// The renderer `--expect-renderer` requires to have presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Expected {
    /// rinch's software renderer.
    Software,
    /// rinch's GPU renderer, on a GPU (not a software Vulkan driver).
    Gpu,
}

impl Expected {
    fn renderer(self) -> deskrinch_test_component::Renderer {
        match self {
            Self::Software => deskrinch_test_component::Renderer::Software,
            Self::Gpu => deskrinch_test_component::Renderer::Gpu,
        }
    }
}

/// Whether every run presented with `expected`; says which did not, and
/// with what.
pub(super) fn check_renderer(expected: Expected, runs: &[report::Run]) -> Outcome {
    let wrong: Vec<String> = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| {
            !run.presented
                .as_ref()
                .is_some_and(|presented| expected.renderer().presented(presented))
        })
        .map(|(index, run)| {
            format!(
                "run {}: {}",
                index + 1,
                presented_name(run.presented.as_ref())
            )
        })
        .collect();
    if wrong.is_empty() {
        println!("measure: every run presented with the {expected:?} renderer, as expected");
        Outcome::Passed
    } else {
        eprintln!(
            "measure: expected the {expected:?} renderer to present, but {}",
            wrong.join("; ")
        );
        Outcome::Failed
    }
}

/// What a run's program said presented.
fn presented_name(presented: Option<&deskrinch_test_component::Presented>) -> String {
    use deskrinch_test_component::Presented;
    match presented {
        None => "it did not say which renderer presented".to_owned(),
        Some(Presented::Software) => "rinch's software renderer presented".to_owned(),
        Some(Presented::Gpu(adapter)) if adapter.is_software() => format!(
            "the GPU renderer presented on {}, a software Vulkan driver",
            adapter.name
        ),
        Some(Presented::Gpu(adapter)) => {
            format!("the GPU renderer presented on {}", adapter.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use deskrinch_test_component::Presented;
    use deskrinch_test_component::log::Adapter;

    use super::*;

    fn run(presented: Option<Presented>) -> report::Run {
        report::Run {
            presented,
            ..report::Run::default()
        }
    }

    fn gpu(device_type: &str) -> Presented {
        Presented::Gpu(Adapter {
            name: "llvmpipe".into(),
            driver: "llvmpipe".into(),
            driver_info: String::new(),
            device_type: device_type.into(),
            backend: "Vulkan".into(),
        })
    }

    #[test]
    fn every_run_must_present_with_the_renderer_expected() {
        let software = [
            run(Some(Presented::Software)),
            run(Some(Presented::Software)),
        ];
        assert_eq!(
            check_renderer(Expected::Software, &software),
            Outcome::Passed
        );
        assert_eq!(check_renderer(Expected::Gpu, &software), Outcome::Failed);
        let mixed = [run(Some(Presented::Software)), run(None)];
        assert_eq!(check_renderer(Expected::Software, &mixed), Outcome::Failed);
        // A software Vulkan driver is not the GPU.
        assert_eq!(
            check_renderer(Expected::Gpu, &[run(Some(gpu("Cpu")))]),
            Outcome::Failed
        );
        assert_eq!(
            check_renderer(Expected::Gpu, &[run(Some(gpu("DiscreteGpu")))]),
            Outcome::Passed
        );
    }

    #[test]
    fn says_what_presented_instead() {
        assert_eq!(
            presented_name(Some(&gpu("Cpu"))),
            "the GPU renderer presented on llvmpipe, a software Vulkan driver"
        );
        assert_eq!(
            presented_name(None),
            "it did not say which renderer presented"
        );
    }
}
