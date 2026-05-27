//! Tauri shell for Hearth.
//!
//! Initialization shape mirrors sc-langpatch's: synchronous `.manage()`
//! call on a fully-built `AppState`, no async work inside `.setup()`.
//! The DB pool open is bridged through a dedicated thread because sqlx
//! is tokio-native and Tauri's runtime hasn't started yet when we're
//! constructing state.

use std::path::PathBuf;

use hearth_core::{BpView, OwnedBlueprint, WishlistEntry};
use hearth_storage::DbPool;
use specta_typescript::Typescript;
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
    /// completes; subsequent calls reuse it.
    sc_data: Mutex<Option<LoadedScData>>,
}

/// `%APPDATA%/hearth/hearth.db` on Windows.
fn db_path() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("hearth").join("hearth.db"))
        .expect("OS data dir not resolvable")
}

/// Open the SQLite pool synchronously before Tauri's runtime starts.
///
/// Runs the async sqlx setup on a dedicated `std::thread` with its own
/// current-thread tokio runtime. The thread gets an 8 MiB stack — sqlx
/// itself doesn't need that much, but every tokio-using thread on
/// Windows wants the headroom and giving it here costs nothing.
fn init_db_pool() -> anyhow::Result<DbPool> {
    std::thread::Builder::new()
        .name("hearth-db-init".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| -> anyhow::Result<DbPool> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(hearth_storage::open(&db_path()))
        })?
        .join()
        .map_err(|panic| {
            let msg = panic
                .downcast_ref::<&'static str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("(no message)");
            anyhow::anyhow!("db-init thread panicked: {msg}")
        })?
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
async fn list_blueprints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BpView>, AppError> {
    let mut guard = state.sc_data.lock().await;

    if guard.is_none() {
        let loaded = LoadedScData::load_async()
            .await
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
    // Open the SQLite pool synchronously BEFORE Tauri starts its runtime.
    // This avoids the race where the frontend would invoke a command
    // before state is managed, and keeps the heavy initialization off
    // Tauri's small-stack worker threads.
    let db = init_db_pool().expect("opening hearth.db");

    let state = AppState {
        db,
        sc_data: Mutex::new(None),
    };

    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        list_blueprints,
        list_owned,
        add_owned,
        remove_owned,
        list_wishlist,
        add_to_wishlist,
        remove_from_wishlist,
    ]);

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
