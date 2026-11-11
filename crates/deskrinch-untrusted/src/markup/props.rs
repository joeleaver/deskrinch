//! Properties of the markup parser over untrusted input.

use std::fmt::Write as _;

use proptest::prelude::*;

use super::*;
use crate::fuzz::config;

/// Pieces real and hostile bodies are made of: the subset's tags, broken
/// tags, references good and bad, control characters and plain text.
const PIECES: &[&str] = &[
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<u>",
    "</u>",
    "<B>",
    "<a href=\"https://example.org/?q=1&amp;r=2\">",
    "<a href='mailto:a@b.c'>",
    "<a href=\"javascript:alert(1)\">",
    "<a href=x>",
    "</a>",
    "<img src=\"/x.png\" alt=\"&lt;b&gt;\"/>",
    "<img>",
    "<br>",
    "<br/>",
    "<p>",
    "</p>",
    "<script>",
    "<!--",
    "-->",
    "<",
    ">",
    "<b",
    "</",
    "&amp;",
    "&lt;",
    "&gt;",
    "&quot;",
    "&apos;",
    "&#60;",
    "&#x3C;",
    "&#0;",
    "&#27;",
    "&#x110000;",
    "&nbsp;",
    "&",
    "&#",
    "\n",
    "\t",
    "\r",
    "\u{1b}",
    "\u{0}",
    "\u{202e}",
    "text",
    " ",
    "é",
    "日本",
];

fn hostile() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..120).prop_map(|p| p.concat())
}

fn bodies() -> impl Strategy<Value = String> {
    prop_oneof![hostile(), any::<String>()]
}

fn options() -> impl Strategy<Value = Options> {
    (any::<bool>(), 0..4096usize, 0..1024usize).prop_map(|(hyperlinks, markup, text)| Options {
        hyperlinks,
        max_markup_bytes: markup,
        max_text_bytes: text,
    })
}

/// The markup that parses back to `body`: every character of its text
/// escaped, its styles and links as tags.
fn to_markup(body: &Body) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let mut out = String::new();
    for span in body.spans() {
        let tags: Vec<(&str, String)> = [
            (span.style.bold, "b"),
            (span.style.italic, "i"),
            (span.style.underline, "u"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, t)| (t, format!("<{t}>")))
        .chain(
            span.link
                .iter()
                .map(|l| ("a", format!("<a href=\"{}\">", escape(l.as_str())))),
        )
        .collect();
        for (_, open) in &tags {
            out.push_str(open);
        }
        out.push_str(&escape(&span.text));
        for (name, _) in tags.iter().rev() {
            let _infallible = write!(out, "</{name}>");
        }
    }
    out
}

proptest! {
    #![proptest_config(config())]

    /// The body is bounded, holds no control characters but line breaks
    /// and tabs, has no empty or redundant spans, and links only to the
    /// allowed schemes.
    #[test]
    fn parsed_bodies_are_bounded_and_clean(markup in bodies(), options in options()) {
        let body = parse(&markup, options);
        prop_assert!(body.plain_text().len() <= options.max_text_bytes);
        for span in body.spans() {
            prop_assert!(!span.text.is_empty());
            prop_assert!(span.text.chars().all(|c| !c.is_control() || c == '\n' || c == '\t'));
            if let Some(link) = &span.link {
                prop_assert!(options.hyperlinks);
                prop_assert_eq!(Link::parse(link.as_str()), Some(link.clone()));
            }
        }
        for pair in body.spans().windows(2) {
            prop_assert!(pair[0].style != pair[1].style || pair[0].link != pair[1].link);
        }
    }

    /// The parsed body is data, not markup: written back out with all of
    /// its text escaped, it parses to itself. Anything in the text that
    /// looks like a tag (a `<` the sender escaped, say) is text, and nothing
    /// in it is interpreted twice.
    #[test]
    fn a_parsed_body_carries_no_markup(markup in bodies(), hyperlinks in any::<bool>()) {
        let options = Options { hyperlinks, max_markup_bytes: usize::MAX, max_text_bytes: 4096 };
        let body = parse(&markup, options);
        let again = parse(&to_markup(&body), Options { max_text_bytes: usize::MAX, ..options });
        prop_assert_eq!(again, body);
    }
}
