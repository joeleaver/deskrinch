//! The measured program's environment: what `measure` sets for every run
//! (rinch's frame lines, the adapter line, the renderer and the shader
//! cache asked for), and, last, what `--env KEY=VALUE` asks for, set on the
//! program itself so that the program KWin sees is the program, and named
//! in the header and each run's notes.

use std::path::Path;
use std::process::Command;

use super::Request;

/// Sets the measured program's environment on `command`: the program, or
/// Valgrind running it.
pub(super) fn environment(command: &mut Command, request: &Request, cache: Option<&Path>) {
    let callers_log = std::env::var("RUST_LOG").ok();
    command
        .env("RINCH_PERF", "1")
        .env(
            "RUST_LOG",
            deskrinch_test_component::log::rust_log(callers_log.as_deref()),
        )
        .env("NO_COLOR", "1");
    if let Some(renderer) = request.renderer {
        command.env("RINCH_RENDERER", renderer.env());
    }
    match cache {
        Some(dir) => command.env("MESA_SHADER_CACHE_DIR", dir),
        // The session's own cache, empty: never one the caller set.
        None => command.env_remove("MESA_SHADER_CACHE_DIR"),
    };
    // Last: what the caller asked for wins.
    command.envs(request.env.iter().map(|(key, value)| (key, value)));
}

/// What `--env` set, for the header and the notes: `None` when nothing.
pub(super) fn environment_set(request: &Request) -> Option<String> {
    (!request.env.is_empty()).then(|| {
        request
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// `KEY=VALUE`, as `--env` takes it: a non-empty key, and the value after
/// the first `=` (which may itself hold `=`).
pub(crate) fn parse_env(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_owned(), value.to_owned())),
        _ => Err(format!("{text:?} is not KEY=VALUE")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn env_takes_key_equals_value() {
        assert_eq!(
            parse_env("MALLOC_ARENA_MAX=1"),
            Ok(("MALLOC_ARENA_MAX".into(), "1".into()))
        );
        assert_eq!(parse_env("A=b=c"), Ok(("A".into(), "b=c".into())));
        assert_eq!(parse_env("EMPTY="), Ok(("EMPTY".into(), String::new())));
        assert!(parse_env("=1").is_err());
        assert!(parse_env("NOVALUE").is_err());
    }

    #[test]
    fn what_was_asked_is_set_last_and_named() {
        let mut request = Request::new("program".into());
        assert_eq!(environment_set(&request), None);
        request.env = vec![
            ("MALLOC_ARENA_MAX".into(), "1".into()),
            ("RINCH_PERF".into(), "0".into()),
        ];
        let mut command = Command::new("program");
        environment(&mut command, &request, None);
        let value = |key: &str| {
            command
                .get_envs()
                .find(|(name, _)| *name == OsStr::new(key))
                .and_then(|(_, value)| value)
                .map(OsStr::to_owned)
        };
        assert_eq!(value("MALLOC_ARENA_MAX"), Some("1".into()));
        assert_eq!(value("RINCH_PERF"), Some("0".into()), "the caller's wins");
        assert_eq!(
            environment_set(&request).as_deref(),
            Some("MALLOC_ARENA_MAX=1 RINCH_PERF=0")
        );
    }
}
