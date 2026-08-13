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

# The e2e image compiles tauler inside itself; the cache mounts that make that
# bearable are BuildKit-only, which is why this is not left to testcontainers.
e2e-image:
    @docker buildx version >/dev/null 2>&1 || { \
        echo "docker buildx is required: the image uses BuildKit cache mounts."; \
        echo "Arch: pacman -S docker-buildx    Debian/Ubuntu: apt install docker-buildx-plugin"; \
        exit 1; \
    }
    docker buildx build -f tauler-e2e/Dockerfile -t tauler-e2e:local --load .

e2e: e2e-image
    cargo test -p tauler-e2e -- --ignored --test-threads=1

install:
    cargo install --path . --locked
    cargo install --path tauler-i3 --locked
    cargo install --path tauler-notify --locked

install-fast:
    cargo install --path . --locked --debug
    cargo install --path tauler-i3 --locked --debug
    cargo install --path tauler-notify --locked --debug
