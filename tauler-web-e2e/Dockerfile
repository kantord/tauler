# The browser the web renderer is compared against.
#
# Pinned, and that is the whole point of the image. ADR 0026 gates on geometry rather than
# pixels precisely because cross-engine pixels cannot be gated — but even the geometry gate
# is a claim about *a* browser: text advance widths come out of the font stack Chrome was
# built with, and Chrome updates roughly monthly. A number measured against whatever
# happened to be installed means nothing the next time it changes. Same argument ADR 0004
# makes for glibc, one layer up.
#
# Unlike `tauler-e2e/Dockerfile` this image does not build tauler: nothing Rust runs inside
# it. The wasm module, the documentation site and the test binary are all built on the host,
# and the site is served to the browser over a socket — so the only thing that has to be
# reproducible in here is Chrome.
#
# `chromedp/headless-shell` rather than a `.deb` from Google's pool: that pool keeps only
# recent releases, so a pinned URL 404s within weeks and the image stops building for a
# reason that has nothing to do with this project. These tags are immutable.
FROM chromedp/headless-shell:151.0.7922.109

# No system fonts are added on purpose. Every font the comparison depends on is served by
# the page out of `assets/fonts/`, so a font arriving here by way of a dependency would be a
# host dependency creeping back in — which is exactly the bug ADR 0026 records for `<Icon>`.

# `--remote-debugging-address` is the load-bearing flag: the default binds to loopback
# inside the container, which the test on the host cannot reach through a published port.
#
# The colour and hinting flags are the same ones `browser.rs` passes, and for the same
# measured reasons — they are repeated here because a container started by hand for
# debugging has to behave like the one the suite starts.
ENTRYPOINT ["/headless-shell/headless-shell"]
CMD [ \
    "--headless", \
    "--no-sandbox", \
    "--disable-gpu", \
    "--remote-debugging-port=9222", \
    "--remote-debugging-address=0.0.0.0", \
    "--force-color-profile=srgb", \
    "--font-render-hinting=none" \
]
EXPOSE 9222
