use super::*;

#[test]
fn a_query_line_holds_one_place_for_the_length() {
    let line = QueryLine::parse("hidden with a query of {} characters").unwrap();
    assert_eq!(line.pattern(), "hidden with a query of {} characters");
    assert!(QueryLine::parse("hidden with a query").is_err());
    assert!(QueryLine::parse("{} of {}").is_err());
}

#[test]
fn a_query_line_reads_the_length_its_line_gives() {
    let query = QueryLine::parse("hidden with a query of {} characters").unwrap();
    let logged =
        "2026-11-14T12:00:00Z  INFO deskrinch_launcher: hidden with a query of 5 characters";
    assert_eq!(query.length(logged), Some(5));
    assert_eq!(
        query.length("hidden with a query of 12 characters"),
        Some(12)
    );
    assert_eq!(query.length("hidden with a query of  characters"), None);
    assert_eq!(query.length("hidden with a query of 5 words"), None);
    assert_eq!(query.length("shown with a query of 5 characters"), None);
    let at_end = QueryLine::parse("query length {}").unwrap();
    assert_eq!(at_end.length("query length 7"), Some(7));
}

#[test]
fn the_text_is_what_a_key_types() {
    assert_eq!(parse_text("plain").unwrap(), "plain");
    assert_eq!(parse_text("a b").unwrap(), "a b");
    assert!(parse_text("").is_err());
    let capital = parse_text("Plain").unwrap_err();
    assert!(capital.contains("'P'"), "{capital}");
}
