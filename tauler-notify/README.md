# tauler-notify

A desktop notification data source for the [tauler](https://crates.io/crates/tauler)
status bar. It is a freedesktop notification daemon that claims `org.freedesktop.Notifications`
on the session bus and emits the current notification list as JSON, leaving the
presentation to your layout. It replaces daemons like dunst or mako.

## Install

```sh
cargo install tauler-notify
```

## Documentation

https://kantord.github.io/tauler/docs/modules/tauler-notify/

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
