install:
    cargo build --release
    install -d ~/.bin ~/.local/bin
    install -m 0755 target/release/pc ~/.bin/pc
    @if command -v codesign >/dev/null 2>&1; then codesign --force --sign - ~/.bin/pc; fi
    # Keep PATHs that prefer ~/.local/bin on the same canonical build.
    ln -sf ~/.bin/pc ~/.local/bin/pc
    # backward-compat: legacy `proactive-context` name points at the same binary
    ln -sf pc ~/.bin/proactive-context
    @echo "Installed to ~/.bin/pc (~/.local/bin/pc and proactive-context link to it)"

loc:
    scripts/check-rust-loc.sh

check: loc
    cargo test --locked
