fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace -- -D warnings

test:
    cargo test --workspace

ci: fmt lint test

docs:
    cargo build -p costae-screenshot
    cargo run -p costae-docgen

install:
    cargo install --path . --locked
    cargo install --path costae-i3 --locked
    cargo install --path costae-notify --locked

install-fast:
    cargo install --path . --locked --debug
    cargo install --path costae-i3 --locked --debug
    cargo install --path costae-notify --locked --debug
