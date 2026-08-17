fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace -- -D warnings

test:
    cargo test --workspace

ci: fmt lint test

# Point every intra-workspace path dependency at the current workspace version.
# The release PR runs this for itself; this recipe is for when it did not.
sync-versions:
    ./scripts/sync-workspace-versions.sh

docs: web
    cargo build -p tauler-screenshot
    cargo run -p tauler-docgen
    cd docs && pnpm install --frozen-lockfile && pnpm run build:css

# The wasm module the docs site loads. Not committed — it is a build product, and it would
# be rewritten on most pull requests that touch `tauler-core` (ADR 0024).
#
# `wasm-bindgen` refuses to process a module built against a different version of its own
# crate, so the CLI version and the `wasm-bindgen` dependency in `tauler-core/Cargo.toml`
# are one number in two places.
WASM_BINDGEN_VERSION := "0.2.126"

web:
    @command -v wasm-bindgen >/dev/null 2>&1 || { \
        echo "wasm-bindgen {{WASM_BINDGEN_VERSION}} is required:"; \
        echo "  cargo install wasm-bindgen-cli --version {{WASM_BINDGEN_VERSION}}"; \
        exit 1; \
    }
    @rustup target list --installed | grep -qx wasm32-unknown-unknown || { \
        echo "the wasm target is required: rustup target add wasm32-unknown-unknown"; \
        exit 1; \
    }
    cargo build -p tauler-web --target wasm32-unknown-unknown --release
    mkdir -p docs/public/tauler
    wasm-bindgen --target web --no-typescript \
        --out-dir docs/public/tauler \
        target/wasm32-unknown-unknown/release/tauler_web.wasm

# The crate boundary is the whole of ADR 0010's "third measurement" — this is the check
# that makes it a claim a compiler enforces rather than a convention.
check-wasm:
    cargo check -p tauler-core --target wasm32-unknown-unknown

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

# The pinned browser the web comparison is measured against.
web-e2e-image:
    @docker buildx version >/dev/null 2>&1 || { \
        echo "docker buildx is required: pacman -S docker-buildx / apt install docker-buildx-plugin"; \
        exit 1; \
    }
    docker buildx build -f tauler-web-e2e/Dockerfile -t tauler-web-e2e:local --load .

# The web renderer against a real browser, over the site a reader actually gets.
#
# `--network host` rather than a published port: the browser has to reach the test's own
# static server on 127.0.0.1, and sharing the host's network is the one arrangement that
# works the same way on a laptop and on a CI runner without a `host.docker.internal` shim.
#
# `--test-threads=1` because each scenario drives the one browser and starts its own server.
web-e2e: docs web-e2e-image
    #!/usr/bin/env bash
    set -euo pipefail
    cd docs && pnpm run build && cd ..
    docker rm -f tauler-web-e2e >/dev/null 2>&1 || true
    docker run -d --rm --name tauler-web-e2e --network host tauler-web-e2e:local >/dev/null
    trap 'docker rm -f tauler-web-e2e >/dev/null 2>&1 || true' EXIT
    # `if/then/break` rather than `[ -n "$ws" ] && break`: under `set -e` an AND-list whose
    # left side fails is the last command of the loop body, and the shell exits the whole
    # recipe on the first attempt instead of retrying.
    ws=""
    for _ in $(seq 1 100); do
        # `/json/version` is pretty-printed, so the space after the colon is not optional
        # to match — a pattern without it silently finds nothing and the wait looks like a
        # browser that never started.
        ws=$(curl -sf http://127.0.0.1:9222/json/version 2>/dev/null \
            | sed -n 's/.*"webSocketDebuggerUrl": *"\([^"]*\)".*/\1/p' || true)
        if [ -n "$ws" ]; then break; fi
        sleep 0.2
    done
    if [ -z "$ws" ]; then
        echo "the pinned browser never answered on 127.0.0.1:9222"
        docker logs tauler-web-e2e || true
        exit 1
    fi
    echo "pinned browser at $ws"
    TAULER_CHROME_WS="$ws" cargo test -p tauler-web-e2e -- --ignored --test-threads=1

install:
    cargo install --path . --locked
    cargo install --path tauler-i3 --locked
    cargo install --path tauler-notify --locked
    cargo install --path tauler-accumulate --locked

install-fast:
    cargo install --path . --locked --debug
    cargo install --path tauler-i3 --locked --debug
    cargo install --path tauler-notify --locked --debug
    cargo install --path tauler-accumulate --locked --debug
