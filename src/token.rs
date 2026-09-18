//! Splits source into BPE tokens — the pieces a model actually sees, rather
//! than the words a human sees.
//!
//! The only invariant that matters: concatenating every span in order
//! reproduces the input byte for byte, so the highlighted output can never
//! silently alter the code it was given.

/// The vocabulary the current generation of models is trained on. Fixed at
/// compile time; `Cargo.toml` embeds this one and no other.
const ENCODING: &str = "o200k_base";

#[derive(Debug, PartialEq, Eq)]
pub enum Span<'a> {
    /// A token made only of whitespace, emitted verbatim.
    Ws(&'a str),
    /// A token holding at least one visible character, classified by jev.
    Tok(&'a str),
}

pub fn split(source: &str) -> Vec<Span<'_>> {
    let bpe = tiktoken::get_encoding(ENCODING).expect("the encoding is compiled in");

    let mut spans = Vec::new();
    let mut start = 0; // first byte not yet emitted
    let mut end = 0; // bytes accounted for so far

    for id in bpe.encode(source) {
        end += bpe.decode(&[id]).len();

        // BPE works on bytes and will cut a multi-byte character in half. Half
        // a character is not a `str`, so hold the piece back and let the next
        // token finish it. The last token always lands on `source.len()`, so
        // nothing is ever left behind.
        if !source.is_char_boundary(end) {
            continue;
        }

        let text = &source[start..end];
        spans.push(if text.chars().all(char::is_whitespace) {
            Span::Ws(text)
        } else {
            Span::Tok(text)
        });
        start = end;
    }

    debug_assert_eq!(start, source.len(), "a token was dropped");
    spans
}

/// The classifiable tokens, in the order jev will be asked about them.
pub fn tokens<'a>(spans: &[Span<'a>]) -> Vec<&'a str> {
    spans
        .iter()
        .filter_map(|s| match s {
            Span::Tok(t) => Some(*t),
            Span::Ws(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: [&str; 9] = [
        "",
        "   ",
        "let mut is_test = true;",
        "\tpublic static void main(String[] a) {}\r\n",
        "\n\n  # a comment\n",
        "no_trailing_newline",
        "  leading and trailing  ",
        "unicode → ident_é = \"héllo wörld\";",
        "→→→ 日本語 🎈🎈",
    ];

    fn rejoin(spans: &[Span<'_>]) -> String {
        spans
            .iter()
            .map(|s| match s {
                Span::Ws(t) | Span::Tok(t) => *t,
            })
            .collect()
    }

    /// Multi-byte characters are the interesting case: a BPE token can end in
    /// the middle of one, and a piece dropped or mangled there would show up
    /// here as a failed round trip.
    #[test]
    fn round_trips() {
        for source in SOURCES {
            let spans = split(source);
            assert_eq!(rejoin(&spans), source, "round trip failed for {source:?}");
        }
    }

    #[test]
    fn spans_are_never_empty_and_classify_by_content() {
        for source in SOURCES {
            for span in split(source) {
                match span {
                    Span::Ws(t) => {
                        assert!(!t.is_empty(), "empty span in {source:?}");
                        assert!(t.chars().all(char::is_whitespace), "{t:?} is not whitespace");
                    }
                    Span::Tok(t) => {
                        assert!(!t.is_empty(), "empty span in {source:?}");
                        assert!(
                            t.chars().any(|c| !c.is_whitespace()),
                            "{t:?} holds nothing to classify"
                        );
                    }
                }
            }
        }
    }

    /// The point of the exercise: one identifier is several tokens, where
    /// splitting on whitespace would have handed jev a single blob.
    #[test]
    fn identifiers_are_broken_into_pieces() {
        let spans = split("is_test");
        assert!(spans.len() > 1, "expected several tokens, got {spans:?}");
        assert_eq!(rejoin(&spans), "is_test");
    }

    #[test]
    fn tokens_drop_whitespace() {
        for span in split("let x = 1;\n\n    y") {
            if let Span::Tok(t) = span {
                assert!(t.chars().any(|c| !c.is_whitespace()));
            }
        }
        assert!(tokens(&split("\n\n   \t")).is_empty());
    }
}
