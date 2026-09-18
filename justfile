default:
    @just --list

# Run the highlighter at http://127.0.0.1:3000
serve:
    cargo run --release

# Run the tests
test:
    cargo test

# Build and run the container at http://127.0.0.1:3000
docker:
    docker build -t syntax_highlighting .
    docker run --rm -p 3000:3000 -e TYPESAFE_API_KEY syntax_highlighting
