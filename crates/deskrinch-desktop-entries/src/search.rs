//! Searching entries by what a user types.
//!
//! # Ranking
//!
//! The query is trimmed and compared case-insensitively (Unicode lowercase)
//! as one string with each of an entry's fields:
//!
//! 1. `Name` ([`MatchField::Name`]);
//! 2. `GenericName` ([`MatchField::GenericName`]);
//! 3. each of `Keywords` ([`MatchField::Keyword`]);
//! 4. the file name of the program `Exec` runs ([`MatchField::Executable`]).
//!
//! Translated fields match in the caller's locale and untranslated. Each
//! comparison gives a [`MatchKind`], best first:
//!
//! 1. the field is the query ([`MatchKind::Exact`]);
//! 2. the field starts with the query ([`MatchKind::Prefix`]);
//! 3. a word of the field starts with the query, words being split at
//!    anything that is not a letter or digit ([`MatchKind::WordPrefix`]);
//! 4. the field contains the query ([`MatchKind::Substring`]).
//!
//! An entry's match is its best `(kind, field)` pair: kind first, then
//! field. So any exact or prefix match ranks above any substring match, and
//! within a kind a name match ranks above a keyword match. Entries are
//! sorted by that pair, then by their translated name (lowercased), then
//! by desktop-file ID, so the order is total and stable. An empty query
//! matches nothing.

use crate::entry::DesktopEntry;
use crate::exec;
use crate::locale::{Locale, Localized};

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

/// An entry that matched a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match<'a> {
    /// The entry.
    pub entry: &'a DesktopEntry,
    /// Its best match.
    pub kind: MatchKind,
    /// The field of its best match.
    pub field: MatchField,
}

/// The entries that match `query`, best first, by the ranking in the
/// module documentation. `locales` chooses the translations matched and the
/// name ties are broken by.
pub fn search<'a>(
    entries: impl IntoIterator<Item = &'a DesktopEntry>,
    query: &str,
    locales: &[Locale],
) -> Vec<Match<'a>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<(Match<'a>, String)> = entries
        .into_iter()
        .filter_map(|entry| {
            let (kind, field) = best_match(entry, &query, locales)?;
            let sort_name = entry.name.get(locales).to_lowercase();
            Some((Match { entry, kind, field }, sort_name))
        })
        .collect();
    matches.sort_by(|(a, a_name), (b, b_name)| {
        (a.kind, a.field, a_name, &a.entry.id).cmp(&(b.kind, b.field, b_name, &b.entry.id))
    });
    matches.into_iter().map(|(m, _)| m).collect()
}

fn best_match(
    entry: &DesktopEntry,
    query: &str,
    locales: &[Locale],
) -> Option<(MatchKind, MatchField)> {
    let strings = |value: &Localized<String>| -> Vec<String> {
        vec![value.get(locales).clone(), value.default.clone()]
    };
    let mut fields: Vec<(MatchField, Vec<String>)> = vec![(MatchField::Name, strings(&entry.name))];
    if let Some(generic) = &entry.generic_name {
        fields.push((MatchField::GenericName, strings(generic)));
    }
    if let Some(keywords) = &entry.keywords {
        let mut all = keywords.get(locales).clone();
        all.extend(keywords.default.iter().cloned());
        fields.push((MatchField::Keyword, all));
    }
    let program = entry
        .application()
        .and_then(|app| app.exec.as_deref())
        .and_then(exec::program);
    if let Some(program) = program {
        let file_name = program.rsplit('/').next().unwrap_or(&program).to_owned();
        fields.push((MatchField::Executable, vec![file_name]));
    }
    fields
        .iter()
        .flat_map(|(field, values)| {
            values
                .iter()
                .filter_map(move |value| Some((match_kind(&value.to_lowercase(), query)?, *field)))
        })
        .min()
}

fn match_kind(value: &str, query: &str) -> Option<MatchKind> {
    if value == query {
        Some(MatchKind::Exact)
    } else if value.starts_with(query) {
        Some(MatchKind::Prefix)
    } else if value
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        Some(MatchKind::WordPrefix)
    } else if value.contains(query) {
        Some(MatchKind::Substring)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::DesktopId;

    fn app(id: &str, name: &str, extra: &str) -> DesktopEntry {
        let text =
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run-{id}\n{extra}");
        let id = DesktopId::new(format!("{id}.desktop")).unwrap();
        DesktopEntry::parse(id, "/x".into(), &text).unwrap()
    }

    fn ranked(entries: &[DesktopEntry], query: &str) -> Vec<String> {
        search(entries, query, &[])
            .iter()
            .map(|m| m.entry.id.app_id().to_owned())
            .collect()
    }

    #[test]
    fn exact_then_prefix_then_word_prefix_then_substring() {
        let entries = [
            app("substring", "Ultraterm", ""),
            app("word", "Super Term", ""),
            app("prefix", "Terminal", ""),
            app("exact", "Term", ""),
        ];
        assert_eq!(
            ranked(&entries, "term"),
            ["exact", "prefix", "word", "substring"]
        );
    }

    #[test]
    fn within_a_kind_name_ranks_above_generic_name_keywords_and_executable() {
        let entries = [
            app("byexec", "Zed", "").with_exec("/usr/bin/foo --x"),
            app("bykeyword", "Yak", "Keywords=foo;\n"),
            app("bygeneric", "Xylo", "GenericName=Foo\n"),
            app("byname", "Foo", ""),
        ];
        assert_eq!(
            ranked(&entries, "foo"),
            ["byname", "bygeneric", "bykeyword", "byexec"]
        );
    }

    #[test]
    fn a_keyword_exact_match_beats_a_name_substring() {
        let entries = [
            app("cobweb", "Cobweb", ""),
            app("browser", "Firefox", "Keywords=Web;"),
        ];
        assert_eq!(ranked(&entries, "web"), ["browser", "cobweb"]);
    }

    #[test]
    fn ties_sort_by_name_then_id() {
        let entries = [
            app("b2", "Beta", ""),
            app("b1", "Beta", ""),
            app("a", "alpha", ""),
        ];
        assert_eq!(ranked(&entries, "a"), ["a", "b1", "b2"]);
    }

    #[test]
    fn is_case_insensitive_and_trims() {
        let entries = [app("files", "Dateien Ä", "")];
        assert_eq!(ranked(&entries, "  dateien ä "), ["files"]);
        assert_eq!(ranked(&entries, "   "), Vec::<String>::new());
    }

    #[test]
    fn matches_translations_and_the_untranslated_value() {
        let entries = [app("files", "Files", "Name[de]=Dateien\n")];
        let de = [Locale::parse("de").unwrap()];
        assert_eq!(search(&entries, "dat", &de).len(), 1);
        assert_eq!(search(&entries, "fil", &de).len(), 1);
        assert_eq!(search(&entries, "dat", &[]), []);
    }

    #[test]
    fn reports_the_best_field() {
        let entries = [app("firefox", "Firefox", "GenericName=Web Browser\n")];
        let found = search(&entries, "browser", &[]);
        assert_eq!(
            (found[0].kind, found[0].field),
            (MatchKind::WordPrefix, MatchField::GenericName)
        );
        let found = search(&entries, "run-fire", &[]);
        assert_eq!(
            (found[0].kind, found[0].field),
            (MatchKind::Prefix, MatchField::Executable)
        );
    }

    impl DesktopEntry {
        fn with_exec(mut self, exec: &str) -> Self {
            if let crate::entry::EntryKind::Application(app) = &mut self.kind {
                app.exec = Some(exec.into());
            }
            self
        }
    }
}
