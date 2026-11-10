//! The body's markup, parsed into styled text the UI draws as it is.
//!
//! The specification's subset is `<b>`, `<i>`, `<u>`, `<a href>` and
//! `<img src alt>`, with XML's character references. The parser is lenient,
//! because real senders are: what is not a well-formed tag or reference is
//! text. What it does with each construct, deliberately:
//!
//! - `<b>`, `<i>`, `<u>` set [`Style`]; they nest, and an unmatched closing
//!   tag is ignored.
//! - `<a href="…">` makes a [`Link`] when hyperlinks are supported and the
//!   target is `http:`, `https:` or `mailto:`; otherwise its text is plain.
//!   A link inside a link is not a second link.
//! - `<img alt="…">` becomes its alternative text: body images are not
//!   supported, so nothing is ever loaded from the markup.
//! - `<br>` is a line break, as most senders mean it.
//! - Any other tag, and comments, are dropped; the text inside is kept.
//! - `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` and numeric references are
//!   decoded; any other `&` is text.
//! - Control characters other than line breaks and tabs are dropped.

use crate::text::{self, Lines};

/// A tag is looked for within this many bytes of its `<`, so a stray `<`
/// costs bounded work.
const MAX_TAG_BYTES: usize = 1024;

/// A parsed body: styled runs of text, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body {
    spans: Vec<Span>,
}

impl Body {
    /// The runs of text, in order. Adjacent runs differ in style or link.
    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Whether the body has no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The text with the styling left out.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// A run of text in one style.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Span {
    /// The text. Never empty; may hold `\n` and `\t`.
    pub text: String,
    /// How the text is emphasised.
    pub style: Style,
    /// Where the text links to, if it is a link.
    pub link: Option<Link>,
}

/// The emphasis of a [`Span`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Style {
    /// Inside `<b>`.
    pub bold: bool,
    /// Inside `<i>`.
    pub italic: bool,
    /// Inside `<u>`.
    pub underline: bool,
}

/// A hyperlink target, known to be an `http:`, `https:` or `mailto:` URL
/// with no whitespace or control characters in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Link(String);

impl Link {
    /// Validates `href` as a link target.
    fn parse(href: &str) -> Option<Self> {
        let href = href.trim();
        if href.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return None;
        }
        let (scheme, rest) = href.split_once(':')?;
        let scheme = scheme.to_ascii_lowercase();
        let valid = match scheme.as_str() {
            "http" | "https" => rest.strip_prefix("//").is_some_and(|r| !r.is_empty()),
            "mailto" => !rest.is_empty(),
            _ => false,
        };
        valid.then(|| Self(href.to_owned()))
    }

    /// The URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the parser may produce, from the advertised capabilities and limits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Options {
    /// Whether `<a>` makes links.
    pub(crate) hyperlinks: bool,
    /// Bytes of markup read.
    pub(crate) max_markup_bytes: usize,
    /// Bytes of text produced.
    pub(crate) max_text_bytes: usize,
}

/// Parses a notification body.
pub(crate) fn parse(markup: &str, options: Options) -> Body {
    let mut rest = &markup[..floor_boundary(markup, options.max_markup_bytes)];
    let mut out = Builder::new(options);
    while !rest.is_empty() && !out.full {
        let consumed = if rest.starts_with('<') {
            if let Some((tag, len)) = Tag::parse(rest) {
                out.tag(&tag);
                len
            } else {
                out.text("<");
                1
            }
        } else if rest.starts_with('&') {
            if let Some((c, len)) = entity(rest) {
                out.text(c.encode_utf8(&mut [0; 4]));
                len
            } else {
                out.text("&");
                1
            }
        } else {
            let len = rest.find(['<', '&']).unwrap_or(rest.len());
            out.text(&rest[..len]);
            len
        };
        rest = &rest[consumed..];
    }
    Body { spans: out.spans }
}

/// Accumulates spans while tracking the open tags.
struct Builder {
    options: Options,
    spans: Vec<Span>,
    used: usize,
    full: bool,
    bold: u32,
    italic: u32,
    underline: u32,
    /// Open `<a>` tags; the link belongs to the outermost.
    anchors: u32,
    link: Option<Link>,
}

impl Builder {
    fn new(options: Options) -> Self {
        Self {
            options,
            spans: Vec::new(),
            used: 0,
            full: false,
            bold: 0,
            italic: 0,
            underline: 0,
            anchors: 0,
            link: None,
        }
    }

    fn style(&self) -> Style {
        Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            underline: self.underline > 0,
        }
    }

    fn text(&mut self, s: &str) {
        let style = self.style();
        let mut piece = String::new();
        let budget = self.options.max_text_bytes - self.used;
        self.full = !text::push_clean(&mut piece, s, budget, Lines::Keep);
        if piece.is_empty() {
            return;
        }
        self.used += piece.len();
        match self.spans.last_mut() {
            Some(last) if last.style == style && last.link == self.link => {
                last.text.push_str(&piece);
            }
            _ => self.spans.push(Span {
                text: piece,
                style,
                link: self.link.clone(),
            }),
        }
    }

    fn tag(&mut self, tag: &Tag<'_>) {
        let counter = match tag.name.as_str() {
            "b" => &mut self.bold,
            "i" => &mut self.italic,
            "u" => &mut self.underline,
            "a" => {
                self.anchor(tag);
                return;
            }
            "img" if !tag.closing => {
                if let Some(alt) = tag.attribute("alt") {
                    self.text(&alt);
                }
                return;
            }
            "br" => {
                self.text("\n");
                return;
            }
            _ => return,
        };
        if tag.self_closing {
            return;
        }
        if tag.closing {
            *counter = counter.saturating_sub(1);
        } else {
            *counter = counter.saturating_add(1);
        }
    }

    fn anchor(&mut self, tag: &Tag<'_>) {
        if tag.self_closing {
            return;
        }
        if tag.closing {
            self.anchors = self.anchors.saturating_sub(1);
            if self.anchors == 0 {
                self.link = None;
            }
            return;
        }
        if self.anchors == 0 && self.options.hyperlinks {
            self.link = tag.attribute("href").as_deref().and_then(Link::parse);
        }
        self.anchors = self.anchors.saturating_add(1);
    }
}

/// A tag, or a comment (which has an empty name).
struct Tag<'a> {
    /// Lowercase.
    name: String,
    closing: bool,
    self_closing: bool,
    /// Raw `(name, value)` pairs; values are still escaped.
    attributes: Vec<(&'a str, &'a str)>,
}

impl<'a> Tag<'a> {
    /// Parses the tag `s` starts with, returning it and its length in bytes.
    fn parse(s: &'a str) -> Option<(Self, usize)> {
        let window = &s[..floor_boundary(s, MAX_TAG_BYTES)];
        if let Some(comment) = window.strip_prefix("<!--") {
            let end = comment.find("-->")?;
            let tag = Self {
                name: String::new(),
                closing: false,
                self_closing: true,
                attributes: Vec::new(),
            };
            return Some((tag, 4 + end + 3));
        }
        let mut cur = Cursor { s: window, at: 1 };
        let closing = cur.eat('/');
        let name = cur.take_while(|c| c.is_ascii_alphanumeric());
        if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return None;
        }
        let mut attributes = Vec::new();
        loop {
            let spaced = !cur.take_while(char::is_whitespace).is_empty();
            if cur.eat('>') {
                break;
            }
            if cur.eat('/') {
                if !cur.eat('>') {
                    return None;
                }
                let tag = Self::new(name, closing, true, attributes);
                return Some((tag, cur.at));
            }
            if !spaced {
                return None;
            }
            let key = cur.take_while(|c| c.is_ascii_alphanumeric() || "-_:".contains(c));
            if key.is_empty() {
                return None;
            }
            cur.take_while(char::is_whitespace);
            if !cur.eat('=') {
                return None;
            }
            cur.take_while(char::is_whitespace);
            let quote = cur.peek().filter(|c| *c == '"' || *c == '\'')?;
            cur.at += 1;
            let value = cur.take_while(|c| c != quote);
            if !cur.eat(quote) {
                return None;
            }
            attributes.push((key, value));
        }
        Some((Self::new(name, closing, false, attributes), cur.at))
    }

    fn new(
        name: &str,
        closing: bool,
        self_closing: bool,
        attributes: Vec<(&'a str, &'a str)>,
    ) -> Self {
        Self {
            name: name.to_ascii_lowercase(),
            closing,
            self_closing,
            attributes,
        }
    }

    /// The decoded value of the first attribute called `name`.
    fn attribute(&self, name: &str) -> Option<String> {
        let (_, raw) = self
            .attributes
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))?;
        let mut out = String::with_capacity(raw.len());
        let mut rest = *raw;
        while let Some(at) = rest.find('&') {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            if let Some((c, len)) = entity(rest) {
                out.push(c);
                rest = &rest[len..];
            } else {
                out.push('&');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);
        Some(out)
    }
}

/// A position in a string being scanned.
struct Cursor<'a> {
    s: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.at..].chars().next()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.at += c.len_utf8();
        }
        found
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let rest = &self.s[self.at..];
        let len = rest.find(|c| !keep(c)).unwrap_or(rest.len());
        self.at += len;
        &rest[..len]
    }
}

/// The largest character boundary of `s` at or below `max`.
fn floor_boundary(s: &str, max: usize) -> usize {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Decodes the character reference `s` starts with (`s` starts with `&`),
/// returning the character and the reference's length in bytes.
fn entity(s: &str) -> Option<(char, usize)> {
    let window = &s[..floor_boundary(s, 12)];
    let end = window.find(';')?;
    let name = &window[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let number = name.strip_prefix('#')?;
            let (digits, radix) = match number.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16),
                None => (number, 10),
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            let code = u32::from_str_radix(digits, radix).ok()?;
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

#[cfg(test)]
mod tests;
