//! Tauri shell for Hearth.
//!
//! Stage 0: minimal builder, no commands, no plugins beyond core.
//! Stage 1 onwards: register Tauri commands here (`list_blueprints`,
//! `toggle_owned`, etc.) wired to `hearth-core` / `hearth-storage`.
//! Stage 4: write owned-blueprints JSON via `hearth-export`.
//! v1.5: sensors module (Game.log tailing) lives in `src/sensors/`.

pub mod sensors;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
