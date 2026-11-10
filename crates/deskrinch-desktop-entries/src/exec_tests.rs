use super::*;
use crate::keyfile::unescape;

fn run(exec: &str, targets: &[Target]) -> Result<Vec<Vec<String>>, ExecError> {
    let fields = Fields {
        name: "My App",
        icon: Some("my-icon"),
        desktop_file: Some(Path::new("/usr/share/applications/my.desktop")),
    };
    let lines = expand(exec, &fields, targets)?;
    Ok(lines
        .into_iter()
        .map(|line| line.into_iter().map(|a| a.into_string().unwrap()).collect())
        .collect())
}

fn one(exec: &str, targets: &[Target]) -> Vec<String> {
    let mut lines = run(exec, targets).unwrap();
    assert_eq!(lines.len(), 1, "{exec:?}");
    lines.remove(0)
}

fn path(p: &str) -> Target {
    Target::Path(p.into())
}

fn uri(u: &str) -> Target {
    Target::Uri(u.into())
}

#[test]
fn splits_on_whitespace() {
    assert_eq!(one("app  -a\t-b", &[]), ["app", "-a", "-b"]);
}

#[test]
fn removes_double_quotes_and_their_escapes() {
    assert_eq!(
        one(r#""/opt/my app/bin" "a \"b\" \`c\` \$d \\e" "\x""#, &[]),
        ["/opt/my app/bin", r#"a "b" `c` $d \e"#, r"\x"]
    );
    assert_eq!(one(r#"app "" x"#, &[]), ["app", "", "x"]);
    assert_eq!(
        one(r#"app pre"quoted mid"post"#, &[]),
        ["app", "prequoted midpost"]
    );
}

#[test]
fn string_escapes_come_before_quoting() {
    // As written in the file: four backslashes for one literal backslash in
    // a quoted argument, and `\\$` for a literal dollar sign.
    let exec = unescape(r#"sh -c "echo \\\\ \\$HOME""#);
    assert_eq!(one(&exec, &[]), ["sh", "-c", r"echo \ $HOME"]);
}

#[test]
fn reads_shell_forms_outside_double_quotes() {
    assert_eq!(
        one(r"app 'single quoted \x' a\ b", &[]),
        ["app", r"single quoted \x", "a b"]
    );
}

#[test]
fn rejects_unterminated_quotes() {
    assert_eq!(run(r#"app "open"#, &[]), Err(ExecError::UnterminatedQuote));
    assert_eq!(run("app 'open", &[]), Err(ExecError::UnterminatedQuote));
    assert_eq!(run(r#"app "a\"#, &[]), Err(ExecError::UnterminatedQuote));
}

#[test]
fn expands_name_icon_and_location() {
    assert_eq!(
        one("app %i --title=%c %c %k", &[]),
        [
            "app",
            "--icon",
            "my-icon",
            "--title=My App",
            "My App",
            "/usr/share/applications/my.desktop"
        ]
    );
    let bare = Fields {
        name: "N",
        icon: None,
        desktop_file: None,
    };
    let line = expand("app %i %k --from=%k", &bare, &[]).unwrap();
    assert_eq!(line, [["app", "--from="]]);
}

#[test]
fn percent_signs() {
    assert_eq!(
        one("app 100%% %%f 50% done", &[]),
        ["app", "100%", "%f", "50%", "done"]
    );
    assert_eq!(run("app %", &[]), Err(ExecError::TrailingPercent));
    assert_eq!(one(r#"sh -c "echo %%""#, &[]), ["sh", "-c", "echo %"]);
}

#[test]
fn drops_deprecated_codes() {
    assert_eq!(one("app %d %D %n %N %v %m -x%d", &[]), ["app", "-x"]);
}

#[test]
fn rejects_unknown_codes() {
    assert_eq!(run("app %z", &[]), Err(ExecError::UnknownFieldCode('z')));
    assert_eq!(
        run("app --a=%Q", &[]),
        Err(ExecError::UnknownFieldCode('Q'))
    );
}

#[test]
fn list_codes_and_icon_stand_alone() {
    assert_eq!(
        run("app --files=%F", &[]),
        Err(ExecError::NotStandalone('F'))
    );
    assert_eq!(run("app x%U", &[]), Err(ExecError::NotStandalone('U')));
    assert_eq!(run("app -%i", &[]), Err(ExecError::NotStandalone('i')));
}

#[test]
fn at_most_one_file_code() {
    assert_eq!(run("app %f %u", &[]), Err(ExecError::MultipleFileCodes));
    assert_eq!(run("app %F %F", &[]), Err(ExecError::MultipleFileCodes));
}

#[test]
fn a_single_file_code_runs_once_per_target() {
    let lines = run("app --open=%f", &[path("/a"), uri("file:///b%20c")]).unwrap();
    assert_eq!(lines, [["app", "--open=/a"], ["app", "--open=/b c"]]);
    assert_eq!(one("app %f", &[]), ["app"]);
    assert_eq!(one("app --open=%u", &[]), ["app", "--open="]);
    let lines = run("app %u", &[uri("https://e.org/x"), path("/p q")]).unwrap();
    assert_eq!(
        lines,
        [["app", "https://e.org/x"], ["app", "file:///p%20q"]]
    );
}

#[test]
fn a_list_code_passes_every_target_once() {
    assert_eq!(
        one("app %F", &[path("/a"), uri("file://localhost/b")]),
        ["app", "/a", "/b"]
    );
    assert_eq!(
        one("app %U --", &[path("/a b"), uri("https://e.org")]),
        ["app", "file:///a%20b", "https://e.org", "--"]
    );
    assert_eq!(one("app %U", &[]), ["app"]);
}

#[test]
fn targets_are_ignored_without_a_file_code() {
    assert_eq!(one("app", &[path("/a"), path("/b")]), ["app"]);
}

#[test]
fn local_file_codes_reject_remote_uris() {
    assert_eq!(
        run("app %F", &[uri("https://e.org")]),
        Err(ExecError::NotALocalFile("https://e.org".into()))
    );
    assert_eq!(
        run("app %f", &[uri("file://host/x")]),
        Err(ExecError::NotALocalFile("file://host/x".into()))
    );
}

#[test]
fn an_expansion_is_one_argument() {
    assert_eq!(
        one("app %f", &[path("/with space/$x")]),
        ["app", "/with space/$x"]
    );
}

#[test]
fn checks_the_program() {
    assert_eq!(run("", &[]), Err(ExecError::Empty));
    assert_eq!(run("%f", &[]), Err(ExecError::Empty));
    assert_eq!(
        run("A=b app", &[]),
        Err(ExecError::InvalidProgram("A=b".into()))
    );
    assert_eq!(one("env A=b app", &[]), ["env", "A=b", "app"]);
}

#[test]
fn finds_the_program() {
    assert_eq!(
        program(r#""/opt/my app/run" %U"#).as_deref(),
        Some("/opt/my app/run")
    );
    assert_eq!(program("firefox %u").as_deref(), Some("firefox"));
    assert_eq!(program("%f"), None);
    assert_eq!(program(""), None);
}

#[test]
fn converts_file_uris() {
    assert_eq!(local_path("file:///a%2Fb/c%23"), Some("/a/b/c#".into()));
    assert_eq!(local_path("file:/x"), Some("/x".into()));
    assert_eq!(local_path("file:///x?q#f"), Some("/x".into()));
    assert_eq!(local_path("https://x"), None);
    assert_eq!(
        file_uri(Path::new("/a b/ü~.-_")),
        "file:///a%20b/%C3%BC~.-_"
    );
}
