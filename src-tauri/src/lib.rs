//! Tauri shell for Hearth.

use std::path::PathBuf;

use hearth_core::{BpView, OwnedBlueprint, WishlistEntry};
use hearth_storage::DbPool;
use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands};
use tokio::sync::{Mutex, OnceCell};

pub mod error;
pub mod sc_loader;
pub mod sensors;

use error::AppError;
use sc_loader::LoadedScData;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    /// Cached SC reference data. Boxed so the full struct never crosses
    /// a small-stack thread boundary (see `sc_loader::load_async`).
    sc_data: Mutex<Option<Box<LoadedScData>>>,
    /// SQLite pool, lazily initialized on first DB-needing command.
    /// Using `tokio::sync::OnceCell` so the pool is created on Tauri's
    /// own tokio runtime (not a temporary one that gets dropped),
    /// and concurrent first-callers don't race.
    db: OnceCell<DbPool>,
}

impl AppState {
    async fn db(&self) -> Result<&DbPool, AppError> {
        self.db
            .get_or_try_init(|| async {
                hearth_storage::open(&db_path())
                    .await
                    .map_err(|e| AppError::Storage(format!("{e:#}")))
            })
            .await
    }
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
    let db = state.db().await?;
    hearth_storage::list_owned(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<OwnedBlueprint, AppError> {
    let db = state.db().await?;
    hearth_storage::add_owned(db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let db = state.db().await?;
    hearth_storage::remove_owned(db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn list_wishlist(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WishlistEntry>, AppError> {
    let db = state.db().await?;
    hearth_storage::list_wishlist(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_to_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<WishlistEntry, AppError> {
    let db = state.db().await?;
    hearth_storage::add_to_wishlist(db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_from_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let db = state.db().await?;
    hearth_storage::remove_from_wishlist(db, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

// ── App setup ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        sc_data: Mutex::new(None),
        db: OnceCell::new(),
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
