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

#[test]
fn a_point_is_on_the_first_output_or_the_one_numbered() {
    assert_eq!(Point::parse("640,700").unwrap().to_string(), "1:640,700");
    assert_eq!(Point::parse("2:12.5,0").unwrap().to_string(), "2:12.5,0");
    for wrong in ["640", "0:1,2", "x:1,2", "1:1,", "-1,2", "1,inf", "1:2:3,4"] {
        assert!(Point::parse(wrong).is_err(), "{wrong:?}");
    }
}

#[test]
fn a_point_past_the_outputs_is_refused_and_the_rest_are_named() {
    let pointer = Pointer {
        place: Point::parse("640,360").unwrap(),
        moves: vec![Point::parse("2:10,700").unwrap()],
    };
    assert!(pointer.check(2).is_ok());
    let past = pointer.check(1).unwrap_err().to_string();
    assert!(
        past.contains("2:10,700") && past.contains("1 (--outputs)"),
        "{past}"
    );
    let names = ["HEADLESS-1".to_owned(), "HEADLESS-2".to_owned()];
    assert!(pointer.steps(&names).is_ok());
    assert!(pointer.steps(&names[..1]).is_err());
}
