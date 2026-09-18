use maud::{html, Markup, DOCTYPE};

use crate::jev::{Classified, TokenClass};
use crate::token::Span;

/// Below this, jev is guessing; the token is still colored but marked as unsure.
const UNSURE: f64 = 0.3;

/// Names the hovered token in full. `JSON.stringify` is what shows a leading
/// space or a newline, which most BPE tokens carry and which is exactly the
/// detail the coloring cannot convey.
const READOUT_SCRIPT: &str = r#"
document.addEventListener('mouseover', e => {
  const out = document.getElementById('readout');
  if (!out) return;
  const tok = e.target.closest && e.target.closest('pre.code span[data-class]');
  out.textContent = tok
    ? JSON.stringify(tok.textContent) + ' · ' + tok.dataset.class
      + ' · ' + tok.dataset.confidence + '% confident'
    : ' ';
});
"#;

/// Loads a file into the textarea, so the same one path runs whether the code
/// was pasted, picked, or dropped on the box. Text only: a file with a NUL byte
/// in it is not source, and the decoder would have turned the rest into
/// replacement chars.
const UPLOAD_SCRIPT: &str = r#"
const codeBox = () => document.querySelector('textarea[name=code]');
const overBox = e => e.target.closest && e.target.closest('textarea[name=code]');

async function loadFile(file) {
  const out = document.getElementById('out');
  const text = await file.text();
  out.innerHTML = '';
  if (text.includes('\u0000')) {
    out.appendChild(Object.assign(document.createElement('p'),
      { className: 'error', textContent: file.name + ' is a binary file — pick a text file.' }));
    return;
  }
  codeBox().value = text;
  document.getElementById('filename').textContent = file.name;
}

document.addEventListener('change', e => {
  const input = e.target.closest && e.target.closest('input[type=file]');
  if (!input || !input.files.length) return;
  const file = input.files[0];
  input.value = '';
  loadFile(file);
});

// A drop only reaches the handler if the default is stopped on dragover first,
// and without that the browser would insert the file path as text instead.
document.addEventListener('dragover', e => {
  if (!overBox(e)) return;
  e.preventDefault();
  codeBox().classList.add('dropping');
});

document.addEventListener('dragleave', e => {
  if (overBox(e)) codeBox().classList.remove('dropping');
});

document.addEventListener('drop', e => {
  if (!overBox(e)) return;
  e.preventDefault();
  codeBox().classList.remove('dropping');
  const file = e.dataTransfer.files[0];
  if (file) loadFile(file);
});
"#;

pub fn page() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Syntax highlighting by jev" }
                link rel="stylesheet" href="/style.css";
                script src="https://unpkg.com/htmx.org@2.0.4" {}
                // Delegated from the document, because htmx replaces #out —
                // and the readout inside it — on every submit.
                script {
                    (maud::PreEscaped(READOUT_SCRIPT))
                    (maud::PreEscaped(UPLOAD_SCRIPT))
                }
            }
            body {
                main {
                    h1 { "Syntax highlighting by jev" }
                    p.lede {
                        "Paste code in any language. A BPE tokenizer cuts it into \
                         the same pieces a model would see, and each piece is sent \
                         to jev, which decides what piece of syntax it is. No \
                         grammar is built in."
                    }
                    form hx-post="/highlight" hx-target="#out" hx-disabled-elt="find button" {
                        textarea name="code" spellcheck="false"
                            placeholder="let mut is_test = true;" {}
                        div.actions {
                            button type="submit" { "Highlight" }
                            label.file {
                                "Upload a file"
                                input type="file" hidden;
                            }
                            span.filename #filename {}
                        }
                    }
                    div #out {}
                }
            }
        }
    }
}

pub fn result(spans: &[Span<'_>], classified: &Classified) -> Markup {
    // Whitespace carries no class; every other span consumes the next answer,
    // which jev.rs guarantees is present for each token it was sent.
    let mut answers = classified.tokens.iter();
    let pieces: Vec<(&str, Option<&TokenClass>)> = spans
        .iter()
        .map(|span| match span {
            Span::Ws(text) => (*text, None),
            Span::Tok(text) => (*text, answers.next()),
        })
        .collect();

    html! {
        p.meta { "language: " strong { (classified.language) } }
        pre.code {
            code {
                @for (text, answer) in &pieces {
                    @match answer {
                        Some(a) => {
                            span class=(css_class(a)) title=(hover(a))
                                data-class=(a.class)
                                data-confidence=(format!("{:.0}", a.confidence * 100.0))
                            { (text) }
                        }
                        None => (text),
                    }
                }
            }
        }
        // Filled in by the hover listener in `page`; holds a space when idle so
        // the block below it does not jump as the pointer moves.
        p.readout #readout { " " }
        details.raw {
            summary { "raw response" }
            pre { code { (classified.raw) } }
        }
    }
}

pub fn error(message: &str) -> Markup {
    html! { p.error { (message) } }
}

fn css_class(a: &TokenClass) -> String {
    let unsure = if a.confidence < UNSURE { " unsure" } else { "" };
    format!("tok-{}{}", a.class, unsure)
}

fn hover(a: &TokenClass) -> String {
    format!("{} · {:.0}% confident", a.class, a.confidence * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::split;

    #[test]
    fn output_preserves_the_source_text() {
        let source = "let  mut\tis_test = \"a <b>\";\n";
        let spans = split(source);
        let classified = Classified {
            language: "rust".into(),
            tokens: crate::token::tokens(&spans)
                .iter()
                .map(|_| TokenClass { class: "keyword", confidence: 0.9 })
                .collect(),
            raw: "{}".into(),
        };

        let html = result(&spans, &classified).into_string();

        // Angle brackets in the source must not become markup. The tokenizer
        // may put each one in a span of its own, so they are checked apart.
        assert!(html.contains("&lt;"));
        assert!(html.contains("&gt;"));
        assert!(!html.contains("<b>"));

        // Stripping the tags gives the original text back, so nothing the user
        // pasted is lost, reordered, or re-spaced by the highlighting.
        let body = html.split("<code>").nth(1).unwrap().split("</code>").next().unwrap();
        let mut text = String::new();
        let mut in_tag = false;
        for c in body.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(c),
                _ => {}
            }
        }
        let text = text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        assert_eq!(text, source);
    }
}
