install:
    cargo build --release
    cp target/release/pc ~/.bin/pc
    codesign --force --sign - ~/.bin/pc
    # backward-compat: legacy `proactive-context` name points at the same binary
    ln -sf pc ~/.bin/proactive-context
    @echo "Installed to ~/.bin/pc (proactive-context -> pc symlink for compat)"
