# Syntax highlighting by jev

Paste any code, get it back highlighted. No language grammar is built in: the
code is cut into BPE tokens — the same pieces a model sees — and jev decides
what each one is: keyword, variable, string, comment, and so on.

## Setup

Needs Rust and [just](https://github.com/casey/just).

```sh
export TYPESAFE_API_KEY=<your key>
just serve
```

Then open http://127.0.0.1:3000. The key stays in the server; the browser never
sees it.

Or run it as a container:

```sh
docker build -t syntax_highlighting .
docker run --rm -p 3000:3000 -e TYPESAFE_API_KEY=$TYPESAFE_API_KEY syntax_highlighting
```

The key is passed in at run time, never baked into the image.

## How it works

```
source text
  -> token.rs   cut into o200k_base BPE tokens; whitespace-only ones are
                kept aside, the rest are what jev is asked about
  -> jev.rs     ONE request: state = { source, tokens }, plus one choice
                question per token and one for the language
  -> render.rs  re-assemble, wrapping each token in its class
```

One request per snippet, not one per token. jev reads the `state` once and
scores every question against it in parallel, so batching is about 12x cheaper
and 10x faster with the same answers
([cookbook](https://docs.typesafe.ai/cookbooks/parallel_questions)). It also
means each token is judged with the rest of the file in view, which is what
lets `is_test` in `let mut is_test` come back as a variable rather than a
keyword.

A BPE tokenizer keeps the pipeline honest — it knows nothing about any language,
so no lexer is smuggling in grammar — while cutting at the same places a model
would. Tokens are usually smaller than words: `is_test` arrives in pieces, and a
piece often carries a leading space, so jev is asked to answer for the whole word
a fragment belongs to. Two rough edges come with it. BPE works on bytes and can
cut a multi-byte character in half, so `token.rs` holds a piece back until the
run lands on a character boundary. And 440 BPE tokens is a good deal less code
than 440 words would be.

Re-assembly is exact either way: whitespace tokens are kept, not discarded, and a
test asserts the rendered text equals the input character for character.

## Limits

jev 1.13 allows 64k tokens per request. A submission is capped at 440 BPE
tokens; above that the page asks for a smaller snippet.

## Files

```
src/token.rs    the BPE split, and the round-trip invariant
src/jev.rs      the request shape, the class list, retry on 429/529
src/render.rs   the page and the highlighted fragment
static/style.css   one color per class
```

## Tests

```sh
just test
```
