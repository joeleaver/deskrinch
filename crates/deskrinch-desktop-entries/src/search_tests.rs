use deskrinch_keyfile::Locale;

use super::*;
use crate::entry::DesktopId;

fn app(id: &str, name: &str, extra: &str) -> DesktopEntry {
    app_for(id, name, extra, &[])
}

/// [`app`], parsed for a session in `locales`.
fn app_for(id: &str, name: &str, extra: &str, locales: &[Locale]) -> DesktopEntry {
    let text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run-{id}\n{extra}");
    let id = DesktopId::new(format!("{id}.desktop")).unwrap();
    DesktopEntry::parse(id, "/x".into(), &text, locales).unwrap()
}

fn ranked(entries: &[DesktopEntry], query: &str) -> Vec<String> {
    search(entries, query)
        .iter()
        .map(|m| m.entry.id.app_id().to_owned())
        .collect()
}

fn how(entries: &[DesktopEntry], query: &str) -> (Coverage, MatchKind, MatchField) {
    let found = search(entries, query);
    (found[0].coverage, found[0].kind, found[0].field)
}

impl DesktopEntry {
    fn with_exec(mut self, exec: &str) -> Self {
        if let crate::entry::EntryKind::Application(app) = &mut self.kind {
            app.exec = Some(exec.into());
        }
        self
    }
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
fn ignores_case_and_diacritics_and_trims() {
    let entries = [app("cafe", "Café Ä", ""), app("street", "Straße", "")];
    assert_eq!(ranked(&entries, "  CAFE a "), ["cafe"]);
    assert_eq!(ranked(&entries, "café"), ["cafe"]);
    assert_eq!(ranked(&entries, "strasse"), ["street"]);
    assert_eq!(ranked(&entries, "   "), Vec::<String>::new());
}

#[test]
fn matches_translations_and_the_untranslated_value() {
    let de = [Locale::parse("de").unwrap()];
    let german = [app_for("files", "Files", "Name[de]=Dateien\n", &de)];
    assert_eq!(search(&german, "dat").len(), 1);
    assert_eq!(search(&german, "fil").len(), 1);
    let untranslated = [app("files", "Files", "Name[de]=Dateien\n")];
    assert_eq!(search(&untranslated, "dat"), []);
}

#[test]
fn matches_the_translated_and_the_untranslated_keywords() {
    let de = [Locale::parse("de_DE").unwrap()];
    let extra = "Keywords=edit;\nKeywords[de]=bearbeiten;\n";
    let german = [app_for("text", "Text", extra, &de)];
    for query in ["bearb", "edi"] {
        assert_eq!(
            how(&german, query),
            (Coverage::Whole, MatchKind::Prefix, MatchField::Keyword),
            "{query}"
        );
    }
    let untranslated = [app("text", "Text", extra)];
    assert_eq!(search(&untranslated, "bearb"), []);
}

#[test]
fn reports_the_best_field() {
    let entries = [app("firefox", "Firefox", "GenericName=Web Browser\n")];
    assert_eq!(
        how(&entries, "browser"),
        (
            Coverage::Whole,
            MatchKind::WordPrefix,
            MatchField::GenericName
        )
    );
    assert_eq!(
        how(&entries, "run-fire"),
        (Coverage::Whole, MatchKind::Prefix, MatchField::Executable)
    );
}

#[test]
fn every_term_must_match_somewhere() {
    let entries = [
        app("code", "Visual Studio Code", ""),
        app("vlc", "VLC media player", "Keywords=video;"),
    ];
    assert_eq!(ranked(&entries, "visual code"), ["code"]);
    assert_eq!(
        how(&entries, "visual code"),
        (Coverage::AllTerms, MatchKind::WordPrefix, MatchField::Name)
    );
    // The terms may match different fields; the worst term decides: here
    // "media" (a word prefix of the name) is worse than "video" (an exact
    // keyword), because kind ranks before field.
    assert_eq!(
        how(&entries, "media video"),
        (Coverage::AllTerms, MatchKind::WordPrefix, MatchField::Name)
    );
    assert_eq!(ranked(&entries, "visual vlc"), Vec::<String>::new());
}

#[test]
fn a_whole_query_match_beats_one_of_all_terms() {
    let entries = [
        app("reversed", "Studio Visual", ""),
        app("code", "Visual Studio Code", ""),
        app("apart", "Studio for Visual Art", ""),
    ];
    // "visual studio" is a prefix of one name, and only found term by term
    // in the others. Those two tie (each has "visual" as a word prefix, its
    // weakest term) and fall back to name order.
    assert_eq!(
        ranked(&entries, "visual studio"),
        ["code", "apart", "reversed"]
    );
    assert_eq!(
        how(&entries, "visual  studio"),
        (Coverage::Whole, MatchKind::Prefix, MatchField::Name)
    );
}

#[test]
fn among_all_terms_matches_the_weakest_term_ranks() {
    let entries = [
        app("substring", "Xmedia Player", ""),
        app("prefix", "Media Player", ""),
    ];
    // "player media": the term "media" is a prefix of one name and only a
    // substring of the other.
    assert_eq!(ranked(&entries, "player media"), ["prefix", "substring"]);
}
