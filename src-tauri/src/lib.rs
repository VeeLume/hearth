//! Tauri shell for Hearth.
//!
//! Stage 0: minimal window with no commands.
//! Stage 1 (current): app state (sqlite pool), Tauri commands for the BP
//!   catalog + ownership + wishlist, specta TypeScript bindings exported
//!   to `src/lib/bindings.ts` in debug builds.
//! Stage 4: write owned-blueprints JSON via `hearth-export`.
//! v1.5: sensors module (Game.log tailing) lives in `src/sensors/`.

use std::path::PathBuf;

use hearth_core::{BpView, OwnedBlueprint, WishlistEntry};
use hearth_storage::DbPool;
use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};

pub mod error;
pub mod sc_loader;
pub mod sensors;

use error::AppError;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    db: DbPool,
}

/// `%APPDATA%/hearth/hearth.db` on Windows.
fn db_path() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("hearth").join("hearth.db"))
        .expect("OS data dir not resolvable")
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn list_blueprints() -> Vec<BpView> {
    // Stage 1: stub. Stage 2 wires the real sc-holotable loader and
    // caches the Datacore on AppState so subsequent calls are cheap.
    sc_loader::load_blueprints()
}

#[tauri::command]
#[specta::specta]
async fn list_owned(state: tauri::State<'_, AppState>) -> Result<Vec<OwnedBlueprint>, AppError> {
    hearth_storage::list_owned(&state.db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<OwnedBlueprint, AppError> {
    hearth_storage::add_owned(&state.db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    hearth_storage::remove_owned(&state.db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn list_wishlist(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WishlistEntry>, AppError> {
    hearth_storage::list_wishlist(&state.db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_to_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<WishlistEntry, AppError> {
    hearth_storage::add_to_wishlist(&state.db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_from_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    hearth_storage::remove_from_wishlist(&state.db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

// ── App setup ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        list_blueprints,
        list_owned,
        add_owned,
        remove_owned,
        list_wishlist,
        add_to_wishlist,
        remove_from_wishlist,
    ]);

    // Export TypeScript bindings in debug builds. Release builds don't need
    // this — bindings are committed to src/lib/bindings.ts.
    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .setup(|app| {
            // Open the SQLite pool asynchronously, then attach to managed
            // state. Failure here is fatal — without storage the app can't
            // do anything useful, so we surface it as a panic on the setup
            // thread (Tauri shows it in the console + crash dialog).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let path = db_path();
                let pool = hearth_storage::open(&path)
                    .await
                    .expect("opening hearth.db");
                handle.manage(AppState { db: pool });
            });
            Ok(())
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
