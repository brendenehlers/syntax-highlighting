mod jev;
mod render;
mod token;

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    code: String,
}

#[tokio::main]
async fn main() {
    let api_key = match std::env::var("TYPESAFE_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            eprintln!("TYPESAFE_API_KEY is not set");
            std::process::exit(1);
        }
    };

    let client = Arc::new(jev::Client::new(api_key).expect("could not build HTTP client"));

    let app = Router::new()
        .route("/", get(|| async { render::page() }))
        .route("/style.css", get(style))
        .route("/highlight", post(highlight))
        .with_state(client);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("could not bind the port");

    println!("listening on http://127.0.0.1:{port}");
    axum::serve(listener, app).await.unwrap();
}

async fn style() -> Response {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../static/style.css"),
    )
        .into_response()
}

async fn highlight(State(client): State<Arc<jev::Client>>, Form(input): Form<Input>) -> Response {
    if input.code.trim().is_empty() {
        return render::error("Paste some code first.").into_response();
    }

    let spans = token::split(&input.code);
    let tokens = token::tokens(&spans);

    if tokens.len() > jev::MAX_TOKENS {
        return render::error(&format!(
            "That is {} tokens. One request holds {} at most — send a smaller snippet.",
            tokens.len(),
            jev::MAX_TOKENS
        ))
        .into_response();
    }

    match client.classify(&input.code, &tokens).await {
        Ok(classified) => render::result(&spans, &classified).into_response(),
        Err(message) => render::error(&message).into_response(),
    }
}
