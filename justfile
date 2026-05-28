install:
    cargo build --release
    cp target/release/proactive-context ~/.bin/proactive-context
    codesign --force --sign - ~/.bin/proactive-context
    @echo "Installed to ~/.bin/proactive-context"
