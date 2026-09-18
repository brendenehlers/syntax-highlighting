//! One request per snippet: jev reads the source once and answers one
//! question per token in parallel. See docs.typesafe.ai/cookbooks/parallel_questions.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Map, Value};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-latest";

/// jev 1.13 allows 64k tokens per request, shared by the state and every
/// question. Everything the questions have in common — the kind descriptions
/// and how to read a fragment — is stated once in the state, which holds a
/// question to about 60 tokens, so this many of them plus the state stays
/// inside `REQUEST_BUDGET` — `a_full_request_fits_the_budget` measures it.
/// Note that these are BPE tokens: 800 of them is a much shorter snippet than
/// 800 words would be.
pub const MAX_TOKENS: usize = 800;

/// The tokens jev 1.13 reads in one request, state and questions together.
pub const REQUEST_BUDGET: usize = 64 * 1024;

/// The syntax kinds jev chooses between. The first field is also the CSS class
/// suffix, so `keyword` renders as `tok-keyword`.
pub const CLASSES: [(&str, &str); 10] = [
    ("keyword", "A word reserved by the language itself, such as if, return, public, let, async, def, func"),
    ("type", "The name of a type, class, struct, interface, or trait, whether built in or user defined"),
    ("function", "The name of a function, method, or macro, at its definition or where it is called"),
    ("variable", "The name of a variable, parameter, field, or constant that the programmer chose"),
    ("string", "Quoted text, or part of it, including the quote marks and any character escapes"),
    ("number", "A numeric literal, in any base, with or without a suffix or decimal point"),
    ("comment", "Text the language ignores, such as a line comment marker and the words after it"),
    ("operator", "A symbol that computes or assigns, such as = + - * / == && -> => |> :="),
    ("punctuation", "A symbol that groups, separates, or ends a statement, such as brackets, braces, parentheses, commas, colons, dots, semicolons"),
    ("other", "None of the above, such as a preprocessor line, an annotation, or plain prose"),
];

const LANGUAGES: [&str; 18] = [
    "rust", "go", "python", "javascript", "typescript", "java", "c", "cpp",
    "csharp", "ruby", "php", "swift", "kotlin", "shell", "sql", "html", "css",
    "other",
];

pub struct Classified {
    pub language: String,
    /// One entry per token, in the order the tokens were sent.
    pub tokens: Vec<TokenClass>,
    /// jev's reply verbatim, so the page can show what was actually answered.
    pub raw: String,
}

pub struct TokenClass {
    pub class: &'static str,
    pub confidence: f64,
}

#[derive(Deserialize)]
struct Answer {
    choice: String,
    #[serde(default)]
    confidence: f64,
}

#[derive(Deserialize)]
struct Reply {
    answers: HashMap<String, Answer>,
}

pub struct Client {
    http: reqwest::Client,
    api_key: String,
}

impl Client {
    pub fn new(api_key: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            api_key,
        })
    }

    pub async fn classify(&self, source: &str, tokens: &[&str]) -> Result<Classified, String> {
        let value = self.send(body(source, tokens)).await?;
        let raw = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        let reply: Reply = serde_json::from_value(value)
            .map_err(|e| format!("could not read jev's answer: {e}"))?;

        let language = reply
            .answers
            .get("language")
            .map(|a| a.choice.clone())
            .unwrap_or_else(|| "other".into());

        // A token jev did not answer for, or answered with a name outside the
        // criteria, falls back to `other` rather than dropping out of the output.
        let tokens = (0..tokens.len())
            .map(|i| match reply.answers.get(&format!("t{i}")) {
                Some(a) => TokenClass {
                    class: CLASSES
                        .iter()
                        .find(|(name, _)| *name == a.choice)
                        .map_or("other", |(name, _)| *name),
                    confidence: a.confidence,
                },
                None => TokenClass { class: "other", confidence: 0.0 },
            })
            .collect();

        Ok(Classified { language, tokens, raw })
    }

    /// The API is transient-failure prone under load; back off and retry.
    async fn send(&self, body: Value) -> Result<Value, String> {
        for attempt in 0..5u32 {
            let res = self
                .http
                .post(ENDPOINT)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("could not reach jev: {e}"))?;

            let status = res.status().as_u16();
            if matches!(status, 429 | 529) {
                tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
                continue;
            }
            if !res.status().is_success() {
                let detail = res.text().await.unwrap_or_default();
                return Err(format!("jev returned {status}: {detail}"));
            }
            return res
                .json::<Value>()
                .await
                .map_err(|e| format!("could not read jev's answer: {e}"));
        }
        Err("jev is rate limited or overloaded; try again in a moment".into())
    }
}

fn body(source: &str, tokens: &[&str]) -> Value {
    let kinds: Map<String, Value> = CLASSES
        .iter()
        .map(|(name, desc)| ((*name).to_string(), json!(desc)))
        .collect();

    // Naming the kinds without describing them again: the descriptions are in
    // the state, which jev reads once, rather than in each of the hundreds of
    // questions that would otherwise carry a copy.
    let choices: Map<String, Value> = CLASSES
        .iter()
        .map(|(name, _)| ((*name).to_string(), Value::Null))
        .collect();

    let mut questions = Map::new();
    questions.insert(
        "language".into(),
        json!({
            "type": "choice",
            "instructions": "Which programming language is the code in `source` written in?",
            "criteria": LANGUAGES
                .iter()
                .map(|l| ((*l).to_string(), Value::Null))
                .collect::<Map<String, Value>>(),
        }),
    );

    for (i, token) in tokens.iter().enumerate() {
        questions.insert(
            format!("t{i}"),
            json!({
                "type": "choice",
                "instructions": format!(
                    "Which kind is `tokens[{i}]`, the text `{token}`?",
                ),
                "criteria": choices,
            }),
        );
    }

    json!({
        "model": MODEL,
        "state": {
            "source": source,
            "tokens": tokens,
            "kinds": kinds,
            "guidance": "`source` is one snippet of code. `tokens` lists the pieces a \
                BPE tokenizer cut it into, in order. Every question but `language` \
                names one piece; read it where it occurs in `source` and answer with \
                the kind of syntax it is, choosing from the kinds described in \
                `kinds`. A piece is often a fragment rather than a whole word, and \
                may carry a leading space; answer for the whole word or symbol the \
                fragment is part of, so every fragment of one word gets the same kind.",
        },
        "questions": questions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_has_one_question_per_token_plus_language() {
        let b = body("let x = 1;", &["let", "x", "=", "1;"]);
        let q = b["questions"].as_object().unwrap();
        assert_eq!(q.len(), 5);
        assert!(q.contains_key("language"));
        assert!(q.contains_key("t3"));
        assert!(q["t2"]["instructions"].as_str().unwrap().contains("`=`"));
        assert_eq!(b["state"]["source"], "let x = 1;");
    }

    /// `MAX_TOKENS` is only safe while a request that full still fits what jev
    /// reads in one pass, so measure a real one rather than trusting the
    /// estimate: a full snippet, split by the same tokenizer the server uses.
    #[test]
    fn a_full_request_fits_the_budget() {
        let line = "    let mut is_test = matches!(kind, Kind::Test); // the flag\n";
        let mut source = String::new();
        let tokens = loop {
            source.push_str(line);
            let spans = crate::token::split(&source);
            if crate::token::tokens(&spans).len() >= MAX_TOKENS {
                break crate::token::tokens(&spans);
            }
        };
        let tokens = &tokens[..MAX_TOKENS];

        let request = body(&source, tokens).to_string();
        let bpe = tiktoken::get_encoding("o200k_base").expect("the encoding is compiled in");
        let used = bpe.encode(&request).len();

        assert!(
            used <= REQUEST_BUDGET,
            "a {MAX_TOKENS}-token snippet asks jev to read {used} tokens, over the \
             {REQUEST_BUDGET} it holds"
        );
    }

    #[test]
    fn classes_and_css_stay_in_step() {
        for (name, _) in CLASSES {
            assert!(color_of(name).is_some(), "no color for {name}");
        }
    }

    /// Two classes that look alike teach the reader nothing, so every pair is
    /// held apart perceptually, and every color is held legible on the panel.
    #[test]
    fn colors_are_distinct_and_legible() {
        const MIN_DIFFERENCE: f64 = 25.0; // CIE76; the just-noticeable step is ~2.3
        const MIN_CONTRAST: f64 = 4.5; // WCAG AA for body text
        const PANEL: &str = "#1b1e24";

        let css = include_str!("../static/style.css");
        assert!(css.contains(PANEL), "the panel color moved; recheck contrast");

        let colors: Vec<(&str, [f64; 3])> = CLASSES
            .iter()
            .map(|(name, _)| (*name, lab(&color_of(name).unwrap())))
            .collect();

        for (i, (a, la)) in colors.iter().enumerate() {
            for (b, lb) in &colors[i + 1..] {
                let d = (0..3).map(|k| (la[k] - lb[k]).powi(2)).sum::<f64>().sqrt();
                assert!(d >= MIN_DIFFERENCE, "{a} and {b} differ by only {d:.1}");
            }
        }

        let bg = luminance(PANEL);
        for (name, _) in CLASSES {
            let c = (luminance(&color_of(name).unwrap()) + 0.05) / (bg + 0.05);
            assert!(c >= MIN_CONTRAST, "{name} contrasts only {c:.2}:1 with the panel");
        }
    }

    /// The hex a class is painted with, read out of the stylesheet itself.
    fn color_of(class: &str) -> Option<String> {
        include_str!("../static/style.css")
            .lines()
            .find(|l| l.trim_start().starts_with(&format!(".tok-{class} ")))
            .and_then(|l| l.split_once('#'))
            .map(|(_, rest)| format!("#{}", &rest[..6]))
    }

    fn channels(hex: &str) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (i, c) in out.iter_mut().enumerate() {
            let v = u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).unwrap() as f64 / 255.0;
            // sRGB companding, so the numbers below are about light, not bytes.
            *c = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        }
        out
    }

    fn luminance(hex: &str) -> f64 {
        let [r, g, b] = channels(hex);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn lab(hex: &str) -> [f64; 3] {
        let [r, g, b] = channels(hex);
        let f = |t: f64| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
        let x = f((0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047);
        let y = f(0.2126 * r + 0.7152 * g + 0.0722 * b);
        let z = f((0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883);
        [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
    }
}
