//! Tauri shell for Hearth.
//!
//! Stage 2 (current): real SC data loading via sc_installs + sc-extract,
//!   cached on `AppState` so the first BP catalog call pays the heavy
//!   Datacore parse cost (~10s) once. Subsequent calls return instantly
//!   from the cached `LoadedScData`.

use std::path::PathBuf;

use hearth_core::{BpView, OwnedBlueprint, WishlistEntry};
use hearth_storage::DbPool;
use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};
use tokio::sync::Mutex;

pub mod error;
pub mod sc_loader;
pub mod sensors;

use error::AppError;
use sc_loader::LoadedScData;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    db: DbPool,
    /// Cached SC reference data. `None` until the first BP-catalog call
    /// completes; subsequent calls reuse it. tokio Mutex so the lock
    /// can be held across the blocking-thread spawn.
    sc_data: Mutex<Option<LoadedScData>>,
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
async fn list_blueprints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BpView>, AppError> {
    let mut guard = state.sc_data.lock().await;

    if guard.is_none() {
        // Heavy: Datacore parse + LocaleMap build. Run on the blocking
        // thread pool so the Tauri runtime thread stays responsive.
        let loaded = tauri::async_runtime::spawn_blocking(LoadedScData::load_blocking)
            .await
            .map_err(|e| AppError::Internal(format!("sc_loader task join: {e}")))?
            .map_err(|e| AppError::NoInstall(format!("{e:#}")))?;
        *guard = Some(loaded);
    }

    Ok(guard
        .as_ref()
        .map(|d| d.blueprints())
        .unwrap_or_default())
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

    // Export TypeScript bindings in debug builds. Release builds use the
    // committed src/lib/bindings.ts as-is.
    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .setup(|app| {
            // Open the SQLite pool asynchronously, then attach to managed
            // state. Failure here is fatal — without storage the app can't
            // do anything useful.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let path = db_path();
                let pool = hearth_storage::open(&path)
                    .await
                    .expect("opening hearth.db");
                handle.manage(AppState {
                    db: pool,
                    sc_data: Mutex::new(None),
                });
            });
            Ok(())
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
