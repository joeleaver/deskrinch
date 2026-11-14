//! What an entry keeps of a translated key: the value for the session's
//! locales, chosen when the entry is parsed (by the specification's
//! matching, [`Localized::get`]), and, for what people search by, the
//! untranslated value as well. Translations for other locales are never
//! read ([`deskrinch_keyfile::KeyFile::parse_for_locales`]).

use deskrinch_keyfile::{Locale, Localized};

/// A translated key people search by (`Name`, `GenericName`, `Keywords`):
/// its value for the session's locales, and the untranslated value when
/// that is another. Search matches both, so that an application is found by
/// its untranslated name in a translated session too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated<T> {
    /// The value for the session's locales: a translation, or the
    /// untranslated value when none matches.
    pub value: T,
    /// The untranslated value, when [`value`](Self::value) is a translation
    /// that differs from it.
    pub untranslated: Option<T>,
}

impl<T: Clone + PartialEq> Translated<T> {
    /// `localized`'s value for `locales`, keeping the untranslated value
    /// beside a translation that differs from it.
    pub(crate) fn resolve(localized: Localized<T>, locales: &[Locale]) -> Self {
        match localized.translation(locales) {
            Some(translation) if *translation != localized.default => Self {
                value: translation.clone(),
                untranslated: Some(localized.default),
            },
            _ => Self {
                value: localized.default,
                untranslated: None,
            },
        }
    }
}

impl<T> Translated<T> {
    /// The values search matches: the session's, then the untranslated one
    /// when it is another.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.value).chain(self.untranslated.as_ref())
    }
}

/// `localized`'s value for `locales`, alone: for a key only the session's
/// value of is ever read (a comment, an icon, an action's name and icon).
pub(crate) fn session_value<T: Clone>(localized: Localized<T>, locales: &[Locale]) -> T {
    localized
        .translation(locales)
        .cloned()
        .unwrap_or(localized.default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(name: &str) -> Locale {
        Locale::parse(name).unwrap()
    }

    /// A value with translations for every form of `sr`, and `de`.
    fn sample() -> Localized<String> {
        let mut localized = Localized::new("default".to_owned());
        for key in ["sr", "sr_RS", "sr@latin", "sr_RS@latin", "de"] {
            localized.translations.push((locale(key), key.to_owned()));
        }
        localized
    }

    #[test]
    fn the_resolved_value_is_the_one_get_gives() {
        for wanted in [
            vec![locale("sr_RS@latin")],
            vec![locale("sr_RS")],
            vec![locale("sr@latin")],
            vec![locale("sr_ME@latin")],
            vec![locale("sr_ME")],
            vec![locale("de_AT.UTF-8")],
            vec![locale("fr_FR")],
            vec![locale("fr"), locale("de_DE")],
            vec![],
        ] {
            let localized = sample();
            let expected = localized.get(&wanted).clone();
            let resolved = Translated::resolve(localized.clone(), &wanted);
            assert_eq!(resolved.value, expected, "{wanted:?}");
            assert_eq!(session_value(localized, &wanted), expected, "{wanted:?}");
        }
    }

    #[test]
    fn the_untranslated_value_is_kept_beside_a_translation_that_differs() {
        let translated = Translated::resolve(sample(), &[locale("de")]);
        assert_eq!(translated.untranslated.as_deref(), Some("default"));
        assert_eq!(translated.values().collect::<Vec<_>>(), ["de", "default"]);
        let untranslated = Translated::resolve(sample(), &[locale("fr")]);
        assert_eq!(untranslated.untranslated, None);
        assert_eq!(untranslated.values().collect::<Vec<_>>(), ["default"]);
    }

    #[test]
    fn a_translation_equal_to_the_untranslated_value_is_kept_once() {
        let mut name = Localized::new("Files".to_owned());
        name.translations.push((locale("de"), "Files".to_owned()));
        assert_eq!(
            Translated::resolve(name, &[locale("de")]).untranslated,
            None
        );
        let mut keywords = Localized::new(vec!["files".to_owned()]);
        keywords
            .translations
            .push((locale("de"), vec!["files".to_owned()]));
        let resolved = Translated::resolve(keywords, &[locale("de")]);
        assert_eq!(resolved.values().count(), 1);
    }

    #[test]
    fn keywords_keep_the_session_s_list_and_the_untranslated_one() {
        let mut keywords = Localized::new(vec!["edit".to_owned()]);
        keywords
            .translations
            .push((locale("de"), vec!["bearbeiten".to_owned()]));
        let german = Translated::resolve(keywords.clone(), &[locale("de_DE")]);
        assert_eq!(
            german.values().flatten().collect::<Vec<_>>(),
            ["bearbeiten", "edit"]
        );
        let french = Translated::resolve(keywords, &[locale("fr")]);
        assert_eq!(french.untranslated, None);
        assert_eq!(french.values().flatten().collect::<Vec<_>>(), ["edit"]);
    }
}
