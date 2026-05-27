# Hearth — common development commands.
#
# Install just with: cargo install just
# Run a recipe with: just <recipe>

# Run the Tauri app with hot-reload Svelte frontend.
dev:
    pnpm tauri dev

# Run all tests across the workspace.
test:
    cargo test --workspace

# Run all checks (cargo fmt, clippy, svelte-check).
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    pnpm check

# Format Rust + frontend code.
fmt:
    cargo fmt --all

# Clippy with warnings as errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Build a release installer (NSIS on Windows).
build:
    pnpm tauri build

# Wipe build artifacts.
clean:
    cargo clean
    rm -rf build .svelte-kit node_modules
