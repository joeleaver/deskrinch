//! Searching entries by what a user types.
//!
//! # Matching
//!
//! The query and every field are folded before comparison: Unicode case
//! folding, compatibility decomposition and removal of diacritical marks,
//! so `cafe` finds "Café" and `strasse` finds "Straße". The fields are:
//!
//! 1. `Name` ([`MatchField::Name`]);
//! 2. `GenericName` ([`MatchField::GenericName`]);
//! 3. each of `Keywords` ([`MatchField::Keyword`]);
//! 4. the file name of the program `Exec` runs ([`MatchField::Executable`]).
//!
//! Translated fields match in the locales the entries were parsed for, and
//! untranslated. A
//! comparison of a text with a field gives a [`MatchKind`], best first:
//!
//! 1. the field is the text ([`MatchKind::Exact`]);
//! 2. the field starts with the text ([`MatchKind::Prefix`]);
//! 3. a word of the field starts with the text, words being split at
//!    anything that is not a letter or digit ([`MatchKind::WordPrefix`]);
//! 4. the field contains the text ([`MatchKind::Substring`]).
//!
//! # Queries of several words
//!
//! The query is split into terms at white space. An entry matches in one
//! of two ways, the first the stronger ([`Coverage`]):
//!
//! 1. [`Coverage::Whole`]: the whole query (its terms joined by single
//!    spaces) matches a field. The entry's match is its best `(kind,
//!    field)` pair over all fields.
//! 2. [`Coverage::AllTerms`], for a query of more than one term: every term
//!    matches some field, not necessarily the same one. Each term's best
//!    `(kind, field)` pair is found, and the entry's match is the worst of
//!    them, since the match is only as good as its weakest term.
//!
//! So `visual code` finds "Visual Studio Code" (both terms prefix the name
//! or a word of it), and `visual studio` ranks it as a whole-query prefix
//! match, above any entry that only has the two words apart.
//!
//! # Ranking
//!
//! Entries are sorted by coverage, then kind, then field, then by their
//! folded translated name, then by desktop-file ID, so the order is total
//! and stable. Kind comes before field: any exact or prefix match ranks
//! above any substring match, and within a kind a name match ranks above a
//! keyword match. An empty query matches nothing.

use crate::entry::DesktopEntry;
use crate::exec;
use crate::fold::fold;
use crate::translated::Translated;

/// How a field matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
    /// The field is the query.
    Exact,
    /// The field starts with the query.
    Prefix,
    /// A word of the field starts with the query.
    WordPrefix,
    /// The field contains the query.
    Substring,
}

/// Which field matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchField {
    /// `Name`.
    Name,
    /// `GenericName`.
    GenericName,
    /// One of `Keywords`.
    Keyword,
    /// The program `Exec` runs.
    Executable,
}

/// How much of the query one field matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Coverage {
    /// The whole query matched one field.
    Whole,
    /// Each term of the query matched some field.
    AllTerms,
}

/// An entry that matched a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match<'a> {
    /// The entry.
    pub entry: &'a DesktopEntry,
    /// How much of the query matched one field.
    pub coverage: Coverage,
    /// The kind of match: the best for the whole query, the worst term's
    /// for all terms.
    pub kind: MatchKind,
    /// The field of that match.
    pub field: MatchField,
}

/// The entries that match `query`, best first, by the rules in the module
/// documentation. The entries' names and keywords are matched as they were
/// resolved for the session's locales, and untranslated; ties are broken by
/// the name shown.
pub fn search<'a>(
    entries: impl IntoIterator<Item = &'a DesktopEntry>,
    query: &str,
) -> Vec<Match<'a>> {
    let folded = fold(query);
    let terms: Vec<&str> = folded.split_whitespace().collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let whole = terms.join(" ");
    let mut matches: Vec<(Match<'a>, String)> = entries
        .into_iter()
        .filter_map(|entry| {
            let fields = fields(entry);
            let (coverage, (kind, field)) = best(&fields, &whole)
                .map(|best| (Coverage::Whole, best))
                .or_else(|| {
                    if terms.len() < 2 {
                        return None;
                    }
                    let worst = terms
                        .iter()
                        .map(|term| best(&fields, term))
                        .collect::<Option<Vec<_>>>()?
                        .into_iter()
                        .max()?;
                    Some((Coverage::AllTerms, worst))
                })?;
            let sort_name = fold(&entry.name.value);
            let found = Match {
                entry,
                coverage,
                kind,
                field,
            };
            Some((found, sort_name))
        })
        .collect();
    matches.sort_by(|(a, a_name), (b, b_name)| {
        (a.coverage, a.kind, a.field, a_name, &a.entry.id).cmp(&(
            b.coverage,
            b.kind,
            b.field,
            b_name,
            &b.entry.id,
        ))
    });
    matches.into_iter().map(|(m, _)| m).collect()
}

/// An entry's searchable fields, folded.
fn fields(entry: &DesktopEntry) -> Vec<(MatchField, String)> {
    let mut fields = Vec::new();
    for name in entry.name.values() {
        fields.push((MatchField::Name, fold(name)));
    }
    for generic in entry.generic_name.iter().flat_map(Translated::values) {
        fields.push((MatchField::GenericName, fold(generic)));
    }
    for keyword in entry.keywords.iter().flat_map(Translated::values).flatten() {
        fields.push((MatchField::Keyword, fold(keyword)));
    }
    let program = entry
        .application()
        .and_then(|app| app.exec.as_deref())
        .and_then(exec::program);
    if let Some(program) = program {
        let file_name = program.rsplit('/').next().unwrap_or(&program);
        fields.push((MatchField::Executable, fold(file_name)));
    }
    fields
}

/// The best `(kind, field)` with which `text` matches any of `fields`.
fn best(fields: &[(MatchField, String)], text: &str) -> Option<(MatchKind, MatchField)> {
    fields
        .iter()
        .filter_map(|(field, value)| Some((match_kind(value, text)?, *field)))
        .min()
}

fn match_kind(value: &str, text: &str) -> Option<MatchKind> {
    if value == text {
        Some(MatchKind::Exact)
    } else if value.starts_with(text) {
        Some(MatchKind::Prefix)
    } else if value
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.starts_with(text))
    {
        Some(MatchKind::WordPrefix)
    } else if value.contains(text) {
        Some(MatchKind::Substring)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
