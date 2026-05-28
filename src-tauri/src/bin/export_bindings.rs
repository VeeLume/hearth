//! Regenerates `src/lib/bindings.ts` from the current Rust command
//! surface without booting Tauri (and without paying the SC-data load
//! cost). Useful in CI / pre-commit and when iterating on commands
//! purely from the editor.
//!
//! Run with `cargo run -p hearth --bin export-bindings`.

fn main() {
    // Default to a path resolved against this crate's manifest dir
    // (src-tauri/), so it doesn't matter what cwd `cargo run` was
    // invoked from.
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/lib/bindings.ts")
            .to_string_lossy()
            .into_owned()
    });
    hearth_lib::export_bindings(&out).expect("exporting TypeScript bindings");
    println!("wrote {out}");
}
