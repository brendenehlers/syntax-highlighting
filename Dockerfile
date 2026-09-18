FROM rust:1-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --user-group app

COPY --from=builder /build/target/release/syntax_highlighting /usr/local/bin/syntax_highlighting

USER app
ENV PORT=3000
EXPOSE 3000
CMD ["syntax_highlighting"]
