//! Folding text for matching: case and diacritics ignored.
//!
//! Text is folded by Unicode full case folding, then compatibility
//! decomposition (NFKD), then removal of nonspacing marks (general category
//! `Mn`). So `Café`, `CAFE` and `café` fold alike, as do `Straße` and
//! `STRASSE`, and the ligature `ﬁ` and `fi`. The data and algorithms are
//! ICU4X's (`icu_casemap`, `icu_normalizer`, `icu_properties`).

use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizerBorrowed;
use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

/// `text`, folded for comparison.
pub(crate) fn fold(text: &str) -> String {
    let folded = CaseMapper::new().fold_string(text);
    let decomposed = DecomposingNormalizerBorrowed::new_nfkd().normalize(&folded);
    let categories = CodePointMapData::<GeneralCategory>::new();
    decomposed
        .chars()
        .filter(|&c| categories.get(c) != GeneralCategory::NonspacingMark)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn ignores_case_and_diacritics() {
        assert_eq!(fold("Café"), "cafe");
        assert_eq!(fold("CAFÉ"), fold("cafe"));
        assert_eq!(fold("Ångström"), "angstrom");
        assert_eq!(fold("Straße"), fold("STRASSE"));
        assert_eq!(fold("ﬁle"), "file");
        assert_eq!(fold("Ελληνικά"), fold("ελληνικα"));
    }

    #[test]
    fn keeps_letters_without_decompositions() {
        // `ø` and `ł` are letters of their own, not `o` and `l` with marks.
        assert_eq!(fold("Søren Łódź"), "søren łodz");
    }
}
