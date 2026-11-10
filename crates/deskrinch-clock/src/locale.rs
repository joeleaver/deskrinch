//! From a POSIX locale name to the Unicode locale whose CLDR conventions
//! format the time.
//!
//! A POSIX name is `language[_TERRITORY][.codeset][@modifier]`, such as
//! `sr_RS.UTF-8@latin`. The language and territory carry over to a Unicode
//! locale unchanged (`sr-RS`); the codeset does not matter, since the result
//! is a Rust string; the modifiers glibc uses for a script or a variant
//! become one (`@latin` makes `sr-Latn-RS`), and the rest (`@euro`, which
//! chose a currency) are dropped.
//!
//! `C`, `POSIX` and their variants (`C.UTF-8`), an unset locale and a name
//! that is not a locale give the root locale `und`, whose CLDR formats are
//! the 24-hour ISO-like ones (`HH:mm`), as the C locale's are.

use icu_locale_core::Locale;

/// The Unicode locale for the POSIX locale `name`, by the rules in this
/// module's documentation.
#[must_use]
pub fn posix_to_unicode(name: &str) -> Locale {
    parse(name)
        .and_then(|tag| Locale::try_from_str(&tag).ok())
        .unwrap_or(Locale::UNKNOWN)
}

/// The BCP 47 tag for `name`, or `None` for the C locale and for a name that
/// is not a locale.
fn parse(name: &str) -> Option<String> {
    let (rest, modifier) = match name.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier)),
        None => (name, None),
    };
    let rest = rest.split_once('.').map_or(rest, |(rest, _codeset)| rest);
    let (language, territory) = match rest.split_once('_') {
        Some((language, territory)) => (language, Some(territory)),
        None => (rest, None),
    };
    let alphabetic = |part: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphabetic())
    };
    if !alphabetic(language, 2..=3) {
        return None;
    }
    let territory_valid =
        |t: &str| alphabetic(t, 2..=2) || (t.len() == 3 && t.bytes().all(|b| b.is_ascii_digit()));
    if territory.is_some_and(|t| !territory_valid(t)) {
        return None;
    }
    let (script, variant) = match modifier {
        Some("latin") => (Some("Latn"), None),
        Some("cyrillic") => (Some("Cyrl"), None),
        Some("devanagari") => (Some("Deva"), None),
        Some("valencia") => (None, Some("valencia")),
        _ => (None, None),
    };
    let mut tag = language.to_owned();
    for part in [script, territory, variant].into_iter().flatten() {
        tag.push('-');
        tag.push_str(part);
    }
    Some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> String {
        posix_to_unicode(name).to_string()
    }

    #[test]
    fn carries_language_and_territory_over() {
        assert_eq!(tag("de_DE.UTF-8"), "de-DE");
        assert_eq!(tag("en_US"), "en-US");
        assert_eq!(tag("pt_BR.utf8"), "pt-BR");
        assert_eq!(tag("fr"), "fr");
        assert_eq!(tag("es_419.UTF-8"), "es-419");
        assert_eq!(tag("fil_PH.UTF-8"), "fil-PH");
    }

    #[test]
    fn turns_script_and_variant_modifiers_into_subtags() {
        assert_eq!(tag("sr_RS.UTF-8@latin"), "sr-Latn-RS");
        assert_eq!(tag("uz_UZ.UTF-8@cyrillic"), "uz-Cyrl-UZ");
        assert_eq!(tag("ca_ES.UTF-8@valencia"), "ca-ES-valencia");
        assert_eq!(tag("de_DE@euro"), "de-DE");
    }

    #[test]
    fn gives_the_root_locale_for_c_and_for_nonsense() {
        for name in [
            "C",
            "C.UTF-8",
            "POSIX",
            "",
            "_DE",
            "de_",
            "d e",
            "/usr/lib/locale/x",
            "de_D1",
        ] {
            assert_eq!(posix_to_unicode(name), Locale::UNKNOWN, "{name:?}");
        }
    }
}
