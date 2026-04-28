install:
    cargo install --path . --locked
    cargo install --path costae-i3 --locked
    cargo install --path costae-notify --locked

install-fast:
    cargo install --path . --locked --debug
    cargo install --path costae-i3 --locked --debug
    cargo install --path costae-notify --locked --debug
