fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace -- -D warnings

test:
    cargo test --workspace

ci: fmt lint test

docs:
    cargo build -p tauler-screenshot
    cargo run -p tauler-docgen

install:
    cargo install --path . --locked
    cargo install --path tauler-i3 --locked
    cargo install --path tauler-notify --locked

install-fast:
    cargo install --path . --locked --debug
    cargo install --path tauler-i3 --locked --debug
    cargo install --path tauler-notify --locked --debug
