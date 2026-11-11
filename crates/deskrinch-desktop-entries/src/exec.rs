//! Turning an `Exec` value into command lines, by the specification's
//! rules.
//!
//! The value has already had the `string` escapes undone (`\\` is one
//! backslash by now). Then:
//!
//! - Arguments are separated by spaces, tabs or newlines. An argument may be
//!   double-quoted; inside, `\"`, `` \` ``, `\$` and `\\` stand for the
//!   character after the backslash, and any other backslash is kept.
//!   Outside double quotes, the shell forms real files use despite the
//!   specification (single quotes, and a backslash escaping the next
//!   character) are read as the shell would, as GLib and KDE do.
//! - Field codes are expanded after quoting is removed, and the result is
//!   never split or scanned again:
//!   - `%f` one local file, `%F` a list of them; a `file:` URI is turned
//!     into its path, and any other URI is an error.
//!   - `%u` one URL, `%U` a list; a path is turned into a `file:` URI.
//!   - `%i` is `--icon <Icon>`, two arguments, or nothing without an icon.
//!   - `%c` is the translated name, `%k` the desktop file's path.
//!   - `%%` is a literal `%`.
//!   - The deprecated `%d`, `%D`, `%n`, `%N`, `%v` and `%m` are removed.
//!   - Any other field code makes the command line invalid.
//! - `%F`, `%U` and `%i` must be arguments of their own; `%f`, `%u`, `%c`
//!   and `%k` may be part of one (`--file=%f`). A field code that is an
//!   argument of its own and expands to nothing removes the argument.
//! - At most one of `%f`, `%F`, `%u`, `%U` may appear. With `%f` or `%u`
//!   and several targets the program is run once per target; with none of
//!   the four, targets are ignored.
//! - The program (the first argument) may not contain `=`.
//!
//! The specification leaves field codes inside quotes undefined; they are
//! expanded like any other, which is what real files written as
//! `sh -c "… %%"` expect.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, percent_encode};

use crate::error::ExecError;

/// What a launch opens: a path or a URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A local file. Give it absolute: it is turned into a `file:` URI for
    /// `%u`, and passed as it is to `%f`.
    Path(PathBuf),
    /// A URI, such as `https://example.org` or `file:///tmp/a%20b`.
    Uri(String),
}

/// The values field codes other than the file codes expand to.
#[derive(Debug, Clone, Copy)]
pub struct Fields<'a> {
    /// `%c`: the translated name.
    pub name: &'a str,
    /// `%i`: the icon, if any: a name or a path.
    pub icon: Option<&'a OsStr>,
    /// `%k`: the desktop file.
    pub desktop_file: Option<&'a Path>,
}

/// A part of an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Code(char),
}

/// The characters a `file:` URI keeps unencoded in a path.
const PATH_KEEP: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Expands `exec` for `targets` into the command lines to run, each a
/// program followed by its arguments: one per target with `%f` or `%u`,
/// otherwise one.
///
/// # Errors
///
/// [`ExecError`] when `exec` breaks the specification's rules, or when a
/// non-local URI is given to `%f`/`%F`.
pub fn expand(
    exec: &str,
    fields: &Fields<'_>,
    targets: &[Target],
) -> Result<Vec<Vec<OsString>>, ExecError> {
    let args = tokenize(exec)?;
    let mut file_code = None;
    for arg in &args {
        let standalone = arg.len() == 1;
        for piece in arg {
            let Piece::Code(code) = *piece else { continue };
            if matches!(code, 'f' | 'F' | 'u' | 'U') && file_code.replace(code).is_some() {
                return Err(ExecError::MultipleFileCodes);
            }
            match code {
                'F' | 'U' | 'i' if !standalone => return Err(ExecError::NotStandalone(code)),
                'f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm' => {}
                other => return Err(ExecError::UnknownFieldCode(other)),
            }
        }
    }
    let expander = Expander {
        fields,
        local: matches!(file_code, Some('f' | 'F')),
    };
    let values = targets
        .iter()
        .map(|target| expander.value(target))
        .collect::<Result<Vec<_>, _>>()?;
    let runs: Vec<&[OsString]> = match file_code {
        Some('f' | 'u') if !values.is_empty() => values.iter().map(std::slice::from_ref).collect(),
        Some(_) => vec![&values],
        None => vec![&[]],
    };
    runs.into_iter()
        .map(|run| expander.command_line(&args, run))
        .collect()
}

/// The program an `Exec` value runs, before any expansion, such as
/// `firefox` or `/usr/bin/foot`. `None` when the value is invalid or its
/// first argument is a field code.
#[must_use]
pub fn program(exec: &str) -> Option<String> {
    let args = tokenize(exec).ok()?;
    let mut program = String::new();
    for piece in args.first()? {
        match piece {
            Piece::Text(text) => program.push_str(text),
            Piece::Code(_) => return None,
        }
    }
    Some(program)
}

struct Expander<'a> {
    fields: &'a Fields<'a>,
    /// The file code is `%f`/`%F` (paths) rather than `%u`/`%U` (URLs).
    local: bool,
}

impl Expander<'_> {
    /// What a target expands to under the command line's file code.
    fn value(&self, target: &Target) -> Result<OsString, ExecError> {
        Ok(match (target, self.local) {
            (Target::Path(path), false) if path.is_absolute() => file_uri(path).into(),
            (Target::Path(path), _) => path.clone().into_os_string(),
            (Target::Uri(uri), true) => local_path(uri)
                .ok_or_else(|| ExecError::NotALocalFile(uri.clone()))?
                .into_os_string(),
            (Target::Uri(uri), false) => uri.into(),
        })
    }

    fn command_line(
        &self,
        args: &[Vec<Piece>],
        files: &[OsString],
    ) -> Result<Vec<OsString>, ExecError> {
        let mut line = Vec::new();
        for arg in args {
            if let [Piece::Code(code)] = arg.as_slice() {
                match code {
                    'f' | 'F' | 'u' | 'U' => line.extend(files.iter().cloned()),
                    'i' => {
                        if let Some(icon) = self.fields.icon.filter(|icon| !icon.is_empty()) {
                            line.push("--icon".into());
                            line.push(icon.into());
                        }
                    }
                    'c' => line.push(self.fields.name.into()),
                    'k' => line.extend(self.fields.desktop_file.map(OsString::from)),
                    _deprecated => {}
                }
                continue;
            }
            let mut out = OsString::new();
            for piece in arg {
                match piece {
                    Piece::Text(text) => out.push(text),
                    Piece::Code('f' | 'u') => out.extend(files.first().map(OsString::as_os_str)),
                    Piece::Code('c') => out.push(self.fields.name),
                    Piece::Code('k') => out.extend(self.fields.desktop_file.map(Path::as_os_str)),
                    Piece::Code(_deprecated) => {}
                }
            }
            line.push(out);
        }
        let program = line.first().ok_or(ExecError::Empty)?;
        if program.as_bytes().contains(&b'=') {
            return Err(ExecError::InvalidProgram(
                program.to_string_lossy().into_owned(),
            ));
        }
        Ok(line)
    }
}

/// Splits `exec` into arguments of text and field codes, removing quoting.
fn tokenize(exec: &str) -> Result<Vec<Vec<Piece>>, ExecError> {
    let mut args = Vec::new();
    let mut arg = Arg::default();
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => arg.finish(&mut args),
            '"' => {
                arg.started = true;
                loop {
                    match chars.next().ok_or(ExecError::UnterminatedQuote)? {
                        '"' => break,
                        '\\' => match chars.next().ok_or(ExecError::UnterminatedQuote)? {
                            c @ ('"' | '`' | '$' | '\\') => arg.push(c),
                            other => {
                                arg.push('\\');
                                arg.push(other);
                            }
                        },
                        '%' => arg.percent(&mut chars)?,
                        c => arg.push(c),
                    }
                }
            }
            '\'' => {
                arg.started = true;
                loop {
                    match chars.next().ok_or(ExecError::UnterminatedQuote)? {
                        '\'' => break,
                        '%' => arg.percent(&mut chars)?,
                        c => arg.push(c),
                    }
                }
            }
            '\\' => arg.push(chars.next().unwrap_or('\\')),
            '%' => arg.percent(&mut chars)?,
            c => arg.push(c),
        }
    }
    arg.finish(&mut args);
    Ok(args)
}

/// An argument being tokenized.
#[derive(Default)]
struct Arg {
    pieces: Vec<Piece>,
    text: String,
    /// Set by a quote, so `""` is an (empty) argument.
    started: bool,
}

impl Arg {
    fn push(&mut self, c: char) {
        self.started = true;
        self.text.push(c);
    }

    /// Reads what follows a `%`.
    fn percent(&mut self, chars: &mut std::str::Chars<'_>) -> Result<(), ExecError> {
        let mut lookahead = chars.clone();
        match lookahead.next() {
            None => Err(ExecError::TrailingPercent),
            Some('%') => {
                chars.next();
                self.push('%');
                Ok(())
            }
            Some(code) if code.is_ascii_alphabetic() => {
                chars.next();
                self.started = true;
                if !self.text.is_empty() {
                    self.pieces
                        .push(Piece::Text(std::mem::take(&mut self.text)));
                }
                self.pieces.push(Piece::Code(code));
                Ok(())
            }
            // Not a field code: a plain `%`.
            Some(_) => {
                self.push('%');
                Ok(())
            }
        }
    }

    fn finish(&mut self, args: &mut Vec<Vec<Piece>>) {
        if !self.text.is_empty() {
            self.pieces
                .push(Piece::Text(std::mem::take(&mut self.text)));
        }
        if self.started {
            let mut pieces = std::mem::take(&mut self.pieces);
            if pieces.is_empty() {
                pieces.push(Piece::Text(String::new()));
            }
            args.push(pieces);
        }
        self.started = false;
    }
}

/// The path of a local `file:` URI (`file:///p`, `file://localhost/p` or
/// `file:/p`), percent-decoded.
pub(crate) fn local_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file:")?;
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => {
            let slash = authority_and_path.find('/')?;
            let (host, path) = authority_and_path.split_at(slash);
            (host.is_empty() || host == "localhost").then_some(path)?
        }
        None => rest.starts_with('/').then_some(rest)?,
    };
    let path = path.split(['?', '#']).next()?;
    let bytes: Vec<u8> = percent_decode_str(path).collect();
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

/// The `file://` URI of an absolute path.
pub(crate) fn file_uri(path: &Path) -> String {
    format!(
        "file://{}",
        percent_encode(path.as_os_str().as_bytes(), PATH_KEEP)
    )
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
