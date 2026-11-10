//! Locales and localised values, matched by the Desktop Entry
//! Specification's rules.
//!
//! A key such as `Name[sr@latin]` carries a locale of the form
//! `lang_COUNTRY@MODIFIER`, where `_COUNTRY` and `@MODIFIER` are optional.
//! For a wanted locale `lang_COUNTRY.ENCODING@MODIFIER` the encoding is
//! dropped and the keys are tried in the order `lang_COUNTRY@MODIFIER`,
//! `lang_COUNTRY`, `lang@MODIFIER`, `lang`, skipping the forms whose parts
//! the wanted locale does not have. When nothing matches, the unlocalised
//! value is used.
//!
//! The caller may prefer several locales in order (the GNU `LANGUAGE`
//! list). Each is tried in full before the next; the unlocalised value comes
//! last.

use std::fmt;

/// A locale, `lang[_COUNTRY][@MODIFIER]`, without its encoding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locale {
    /// The language, such as `sr`.
    pub lang: String,
    /// The country, such as `RS`.
    pub country: Option<String>,
    /// The modifier, such as `latin`.
    pub modifier: Option<String>,
}

impl Locale {
    /// Parses a POSIX locale name such as `de_DE.UTF-8@euro`, dropping the
    /// encoding. `C`, `POSIX` and their variants (`C.UTF-8`) name no
    /// language and give `None`, as does an empty or malformed name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let (rest, modifier) = match name.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier)),
            None => (name, None),
        };
        let rest = rest.split_once('.').map_or(rest, |(rest, _encoding)| rest);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country)),
            None => (rest, None),
        };
        let valid =
            |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric());
        if !valid(lang) || lang == "C" || lang == "POSIX" {
            return None;
        }
        if country.is_some_and(|c| !valid(c)) || modifier.is_some_and(|m| !valid(m)) {
            return None;
        }
        Some(Self {
            lang: lang.to_owned(),
            country: country.map(str::to_owned),
            modifier: modifier.map(str::to_owned),
        })
    }

    /// The locales a key may carry to match this one, best first, as the
    /// specification orders them.
    fn candidates(&self) -> Vec<Self> {
        let form = |country: bool, modifier: bool| Self {
            lang: self.lang.clone(),
            country: if country { self.country.clone() } else { None },
            modifier: if modifier {
                self.modifier.clone()
            } else {
                None
            },
        };
        let (has_country, has_modifier) = (self.country.is_some(), self.modifier.is_some());
        let mut forms = Vec::with_capacity(4);
        if has_country && has_modifier {
            forms.push(form(true, true));
        }
        if has_country {
            forms.push(form(true, false));
        }
        if has_modifier {
            forms.push(form(false, true));
        }
        forms.push(form(false, false));
        forms
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lang)?;
        if let Some(country) = &self.country {
            write!(f, "_{country}")?;
        }
        if let Some(modifier) = &self.modifier {
            write!(f, "@{modifier}")?;
        }
        Ok(())
    }
}

/// A value with its translations: a `localestring` key, or a list of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localized<T> {
    /// The value of the key without a locale.
    pub default: T,
    /// The values of the same key with a locale, in file order.
    pub translations: Vec<(Locale, T)>,
}

impl<T> Localized<T> {
    /// A value with no translations.
    pub fn new(default: T) -> Self {
        Self {
            default,
            translations: Vec::new(),
        }
    }

    /// The value for the first of `locales` that has one, by the
    /// specification's matching order, or the unlocalised value.
    pub fn get(&self, locales: &[Locale]) -> &T {
        locales
            .iter()
            .flat_map(Locale::candidates)
            .find_map(|wanted| {
                self.translations
                    .iter()
                    .find(|(locale, _)| *locale == wanted)
                    .map(|(_, value)| value)
            })
            .unwrap_or(&self.default)
    }

    /// Every value: the unlocalised one, then the translations.
    pub fn all(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.default).chain(self.translations.iter().map(|(_, value)| value))
    }
}

/// A `localestring` value.
pub type LocalizedString = Localized<String>;
/// A list of `localestring` values, such as `Keywords`.
pub type LocalizedList = Localized<Vec<String>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(name: &str) -> Locale {
        Locale::parse(name).unwrap()
    }

    #[test]
    fn parses_posix_names_and_drops_the_encoding() {
        assert_eq!(
            locale("sr_RS.UTF-8@latin"),
            Locale {
                lang: "sr".into(),
                country: Some("RS".into()),
                modifier: Some("latin".into()),
            }
        );
        assert_eq!(locale("de").to_string(), "de");
        assert_eq!(locale("pt_BR.utf8").to_string(), "pt_BR");
        for none in ["", "C", "C.UTF-8", "POSIX", "_DE", "de_", "de@", "d e"] {
            assert_eq!(Locale::parse(none), None, "{none:?}");
        }
    }

    fn sample() -> LocalizedString {
        let mut value = LocalizedString::new("default".into());
        for key in ["sr", "sr_RS", "sr@latin", "sr_RS@latin", "de"] {
            value.translations.push((locale(key), key.into()));
        }
        value
    }

    #[test]
    fn matches_in_the_specification_order() {
        let value = sample();
        assert_eq!(value.get(&[locale("sr_RS@latin")]), "sr_RS@latin");
        assert_eq!(value.get(&[locale("sr_RS")]), "sr_RS");
        assert_eq!(value.get(&[locale("sr@latin")]), "sr@latin");
        assert_eq!(value.get(&[locale("sr_ME@latin")]), "sr@latin");
        assert_eq!(value.get(&[locale("sr_ME")]), "sr");
        assert_eq!(value.get(&[locale("de_AT.UTF-8")]), "de");
        assert_eq!(value.get(&[locale("fr_FR")]), "default");
        assert_eq!(value.get(&[]), "default");
    }

    #[test]
    fn a_wanted_locale_never_matches_a_more_specific_key() {
        let mut value = LocalizedString::new("default".into());
        value.translations.push((locale("sr_RS"), "sr_RS".into()));
        value
            .translations
            .push((locale("sr@latin"), "sr@latin".into()));
        assert_eq!(value.get(&[locale("sr")]), "default");
    }

    #[test]
    fn preferred_locales_are_tried_in_order() {
        let value = sample();
        assert_eq!(value.get(&[locale("fr"), locale("de_DE")]), "de");
        assert_eq!(value.get(&[locale("de_DE"), locale("sr")]), "de");
    }
}
