//! What an entry keeps of a translated key: the value for the session's
//! locales, chosen when the entry is parsed (by the specification's
//! matching, [`Localized::get`]), and, for what people search by, the
//! untranslated value as well. Translations for other locales are never
//! read ([`deskrinch_keyfile::KeyFile::parse_for_locales`]).

use deskrinch_keyfile::{Locale, Localized};

/// A `Name` or `GenericName`: the text shown in the session's locales, and
/// the untranslated text when that is another. Search matches both, so
/// that an application is found by its untranslated name in a translated
/// session too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated {
    /// The text for the session's locales: a translation, or the
    /// untranslated text when none matches.
    pub text: String,
    /// The untranslated text, when [`text`](Self::text) is a translation
    /// that differs from it.
    pub untranslated: Option<String>,
}

impl Translated {
    /// `value`'s text for `locales`, keeping the untranslated text beside
    /// a translation that differs from it.
    pub(crate) fn resolve(value: Localized<String>, locales: &[Locale]) -> Self {
        match value.translation(locales) {
            Some(translation) if *translation != value.default => Self {
                text: translation.clone(),
                untranslated: Some(value.default),
            },
            _ => Self {
                text: value.default,
                untranslated: None,
            },
        }
    }

    /// The texts search matches: the shown one, then the untranslated one
    /// when it is another.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.text.as_str()).chain(self.untranslated.as_deref())
    }
}

/// `Keywords`: the list for the session's locales, when one of them has
/// its own, and the untranslated list. Search matches both, as for names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keywords {
    /// The list of the first of the session's locales that has one; `None`
    /// when none has.
    pub best: Option<Vec<String>>,
    /// The untranslated list.
    pub default: Vec<String>,
}

impl Keywords {
    /// `value`'s lists for `locales`.
    pub(crate) fn resolve(value: Localized<Vec<String>>, locales: &[Locale]) -> Self {
        Self {
            best: value.translation(locales).cloned(),
            default: value.default,
        }
    }

    /// Every keyword search matches: the best list's, then the
    /// untranslated list's, each list once.
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.best
            .iter()
            .flatten()
            .chain(&self.default)
            .map(String::as_str)
    }
}

/// `value` for `locales`, alone: for what only the session's text of is
/// ever read (a comment, an icon, an action's name and icon).
pub(crate) fn resolve<T: Clone>(value: Localized<T>, locales: &[Locale]) -> T {
    value.translation(locales).cloned().unwrap_or(value.default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(name: &str) -> Locale {
        Locale::parse(name).unwrap()
    }

    /// A value with translations for every form of `sr`, and `de`.
    fn sample() -> Localized<String> {
        let mut value = Localized::new("default".to_owned());
        for key in ["sr", "sr_RS", "sr@latin", "sr_RS@latin", "de"] {
            value.translations.push((locale(key), key.to_owned()));
        }
        value
    }

    #[test]
    fn the_resolved_text_is_the_one_get_gives() {
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
            let value = sample();
            let expected = value.get(&wanted).clone();
            assert_eq!(
                Translated::resolve(value.clone(), &wanted).text,
                expected,
                "{wanted:?}"
            );
            assert_eq!(resolve(value, &wanted), expected, "{wanted:?}");
        }
    }

    #[test]
    fn the_untranslated_text_is_kept_beside_a_translation_that_differs() {
        let translated = Translated::resolve(sample(), &[locale("de")]);
        assert_eq!(translated.untranslated.as_deref(), Some("default"));
        assert_eq!(translated.texts().collect::<Vec<_>>(), ["de", "default"]);
        let untranslated = Translated::resolve(sample(), &[locale("fr")]);
        assert_eq!(untranslated.untranslated, None);
        assert_eq!(untranslated.texts().collect::<Vec<_>>(), ["default"]);
        let mut same = Localized::new("Files".to_owned());
        same.translations.push((locale("de"), "Files".to_owned()));
        assert_eq!(
            Translated::resolve(same, &[locale("de")]).untranslated,
            None
        );
    }

    #[test]
    fn keywords_keep_the_best_list_and_the_untranslated_one_once_each() {
        let mut value = Localized::new(vec!["edit".to_owned()]);
        value
            .translations
            .push((locale("de"), vec!["bearbeiten".to_owned()]));
        let german = Keywords::resolve(value.clone(), &[locale("de_DE")]);
        assert_eq!(german.all().collect::<Vec<_>>(), ["bearbeiten", "edit"]);
        let french = Keywords::resolve(value, &[locale("fr")]);
        assert_eq!(french.best, None);
        assert_eq!(french.all().collect::<Vec<_>>(), ["edit"]);
    }
}
