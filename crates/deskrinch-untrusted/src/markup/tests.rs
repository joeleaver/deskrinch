use super::*;

const OPTIONS: Options = Options {
    hyperlinks: true,
    max_markup_bytes: 4096,
    max_text_bytes: 1024,
};

fn spans(markup: &str) -> Vec<(String, Style, Option<String>)> {
    spans_with(markup, OPTIONS)
}

fn spans_with(markup: &str, options: Options) -> Vec<(String, Style, Option<String>)> {
    parse(markup, options)
        .spans()
        .iter()
        .map(|s| {
            (
                s.text.clone(),
                s.style,
                s.link.as_ref().map(|l| l.as_str().to_owned()),
            )
        })
        .collect()
}

fn plain(markup: &str) -> String {
    parse(markup, OPTIONS).plain_text()
}

const NONE: Style = Style {
    bold: false,
    italic: false,
    underline: false,
};
const BOLD: Style = Style { bold: true, ..NONE };

#[test]
fn plain_text_is_one_span() {
    assert_eq!(
        spans("hello world"),
        vec![("hello world".into(), NONE, None)]
    );
    assert!(parse("", OPTIONS).is_empty());
}

#[test]
fn bold_italic_underline_nest() {
    let got = spans("a<b>b<i>c<u>d</u></i></b>e");
    let style = |bold, italic, underline| Style {
        bold,
        italic,
        underline,
    };
    assert_eq!(
        got,
        vec![
            ("a".into(), NONE, None),
            ("b".into(), style(true, false, false), None),
            ("c".into(), style(true, true, false), None),
            ("d".into(), style(true, true, true), None),
            ("e".into(), NONE, None),
        ]
    );
}

#[test]
fn tags_are_case_insensitive_and_unmatched_closers_ignored() {
    assert_eq!(
        spans("</b>x<B>y</B>"),
        vec![("x".into(), NONE, None), ("y".into(), BOLD, None)]
    );
}

#[test]
fn unclosed_bold_runs_to_the_end() {
    assert_eq!(spans("<b>x"), vec![("x".into(), BOLD, None)]);
}

#[test]
fn links_with_allowed_schemes_are_links() {
    assert_eq!(
        spans(r#"see <a href="https://example.org/a?b=1&amp;c=2">this</a>."#),
        vec![
            ("see ".into(), NONE, None),
            (
                "this".into(),
                NONE,
                Some("https://example.org/a?b=1&c=2".into())
            ),
            (".".into(), NONE, None),
        ]
    );
    assert_eq!(
        spans("<a href='mailto:a@b.c'>mail</a>"),
        vec![("mail".into(), NONE, Some("mailto:a@b.c".into()))]
    );
}

#[test]
fn links_with_other_schemes_are_plain_text() {
    for href in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,x",
        "http:",
        "https://",
        "https://a b",
        "",
    ] {
        let markup = format!(r#"<a href="{href}">x</a>"#);
        assert_eq!(spans(&markup), vec![("x".into(), NONE, None)], "{href}");
    }
}

#[test]
fn links_are_plain_text_without_the_capability() {
    let options = Options {
        hyperlinks: false,
        ..OPTIONS
    };
    assert_eq!(
        spans_with(r#"<a href="https://e.org">x</a>"#, options),
        vec![("x".into(), NONE, None)]
    );
}

#[test]
fn a_link_inside_a_link_keeps_the_outer_target() {
    let got = spans(r#"<a href="https://a.org">x<a href="https://b.org">y</a>z</a>w"#);
    assert_eq!(
        got,
        vec![
            ("xyz".into(), NONE, Some("https://a.org".into())),
            ("w".into(), NONE, None)
        ]
    );
}

#[test]
fn images_become_their_alt_text() {
    assert_eq!(
        plain(r#"a<img src="/x.png" alt="[cat &amp; dog]"/>b"#),
        "a[cat & dog]b"
    );
    assert_eq!(plain(r#"a<img src="/x.png">b"#), "ab");
}

#[test]
fn br_is_a_line_break() {
    assert_eq!(plain("a<br>b<br/>c<BR />d"), "a\nb\nc\nd");
}

#[test]
fn other_tags_and_comments_are_dropped_keeping_text() {
    assert_eq!(
        plain("<p>x</p><script>alert(1)</script><span style=\"c\">y</span>"),
        "xalert(1)y"
    );
    assert_eq!(plain("a<!-- <b>hidden</b> -->b"), "ab");
}

#[test]
fn malformed_tags_are_text() {
    assert_eq!(plain("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
    assert_eq!(plain("<3 love"), "<3 love");
    assert_eq!(plain("<b"), "<b");
    assert_eq!(plain("<a href=x>y</a>"), "<a href=x>y");
    assert_eq!(plain("<a href=\"x>y"), "<a href=\"x>y");
    assert_eq!(plain("<!-- open"), "<!-- open");
    assert_eq!(plain("<b/x>"), "<b/x>");
}

#[test]
fn entities_are_decoded_and_unknown_ones_are_text() {
    assert_eq!(
        plain("&lt;b&gt; &amp; &quot;&apos; &#65;&#x42;&#X43;"),
        "<b> & \"' ABC"
    );
    assert_eq!(
        plain("AT&T &nbsp; &#; &#x; &#+1; &#xZZ; &#1114112;"),
        "AT&T &nbsp; &#; &#x; &#+1; &#xZZ; &#1114112;"
    );
}

#[test]
fn escaped_tags_stay_text() {
    assert_eq!(
        spans("&lt;b&gt;x&lt;/b&gt;"),
        vec![("<b>x</b>".into(), NONE, None)]
    );
}

#[test]
fn control_characters_are_dropped_even_as_references() {
    assert_eq!(plain("a\u{1b}[1mb&#27;c&#10;d"), "a[1mbc\nd");
}

#[test]
fn text_is_bounded() {
    let options = Options {
        max_text_bytes: 5,
        ..OPTIONS
    };
    assert_eq!(
        spans_with("<b>abc</b>defgh", options),
        vec![("abc".into(), BOLD, None), ("de".into(), NONE, None)]
    );
}

#[test]
fn markup_is_bounded_before_parsing() {
    let options = Options {
        max_markup_bytes: 4,
        ..OPTIONS
    };
    // The cut falls inside "é" (bytes 3 and 4); it moves back to a boundary.
    assert_eq!(
        spans_with("abcé<b>x</b>", options),
        vec![("abc".into(), NONE, None)]
    );
}

#[test]
fn many_stray_angle_brackets_are_cheap_and_literal() {
    let markup = "<a".repeat(10_000);
    let options = Options {
        max_markup_bytes: usize::MAX,
        max_text_bytes: usize::MAX,
        ..OPTIONS
    };
    assert_eq!(parse(&markup, options).plain_text(), markup);
}
