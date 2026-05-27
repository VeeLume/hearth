//! Tauri shell for Hearth.

use std::path::PathBuf;

use hearth_core::{BpView, ChannelGroup, OwnedBlueprint, WishlistEntry};
use hearth_storage::{DbPool, Scope};
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
    /// Cached SC reference data. Boxed because the unboxed struct
    /// overflows the small-stack tokio receiver mid-move (see
    /// `sc_loader` docs).
    ///
    /// One entry per `ChannelGroup` — PU and Test are loaded
    /// independently so switching between them later doesn't trash
    /// the other's cache. Stage 2 only loads the active group on
    /// first call; Stage 3 wires a UI switch.
    sc_data: Mutex<std::collections::HashMap<ChannelGroup, Box<LoadedScData>>>,
    /// SQLite pool, lazily initialized on first DB-needing command on
    /// Tauri's own tokio runtime.
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

    /// Returns the loaded data for the currently-active channel group.
    /// In Stage 2 there's only ever one — whichever was loaded first
    /// (the highest-priority install per `sc_loader::load_inner`).
    /// Loads on demand if not present.
    async fn loaded(&self) -> Result<LoadedRef<'_>, AppError> {
        let mut guard = self.sc_data.lock().await;
        if guard.is_empty() {
            let loaded = LoadedScData::load_async()
                .await
                .map_err(|e| AppError::NoInstall(format!("{e:#}")))?;
            guard.insert(loaded.channel_group, loaded);
        }
        Ok(LoadedRef { guard })
    }
}

/// RAII handle to the active loaded data. Drops the lock when it goes
/// out of scope. Tauri commands borrow through this for the duration
/// of their await.
struct LoadedRef<'a> {
    guard: tokio::sync::MutexGuard<'a, std::collections::HashMap<ChannelGroup, Box<LoadedScData>>>,
}

impl<'a> LoadedRef<'a> {
    fn data(&self) -> &LoadedScData {
        // Pick any (Stage 2 has at most one); deterministic for Stage 3
        // when we switch by storing an explicit "active" group on state.
        self.guard.values().next().expect("loaded data present")
    }

    fn scope(&self) -> Scope<'_> {
        let d = self.data();
        Scope::new(d.channel_group, d.account_id())
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
    let loaded = state.loaded().await?;
    Ok(loaded.data().blueprints())
}

#[tauri::command]
#[specta::specta]
async fn list_owned(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<OwnedBlueprint>, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::list_owned(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<OwnedBlueprint, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::add_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::remove_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn list_wishlist(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WishlistEntry>, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::list_wishlist(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn add_to_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<WishlistEntry, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::add_to_wishlist(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
async fn remove_from_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let loaded = state.loaded().await?;
    let scope = loaded.scope();
    let db = state.db().await?;
    hearth_storage::remove_from_wishlist(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Surface the active channel + group + account to the UI so it can
/// show "PU (LIVE)" or "Test (PTU)" badges and (eventually) drive a
/// channel switcher.
#[tauri::command]
#[specta::specta]
async fn active_scope(state: tauri::State<'_, AppState>) -> Result<ActiveScope, AppError> {
    let loaded = state.loaded().await?;
    let d = loaded.data();
    Ok(ActiveScope {
        channel: d.channel.display_name().to_string(),
        channel_group: d.channel_group,
        account_id: d.account_id().to_string(),
    })
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct ActiveScope {
    channel: String,
    channel_group: ChannelGroup,
    account_id: String,
}

// ── App setup ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        sc_data: Mutex::new(std::collections::HashMap::new()),
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
        active_scope,
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
