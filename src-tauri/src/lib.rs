//! Tauri shell for Hearth.

use std::collections::HashMap;
use std::path::PathBuf;

use hearth_core::{Account, BpView, OwnedBlueprint, Platform, RecordId, WishlistEntry};
use hearth_storage::{DbPool, Scope};
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri_specta::{Builder, collect_commands};
use tokio::sync::{Mutex, OnceCell};

pub mod error;
pub mod identity;
pub mod sc_loader;
pub mod sensors;

use error::AppError;
use sc_loader::LoadedScData;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    /// Cached SC reference data per platform (Prod / Ptu loaded
    /// independently, lazily on first BP-catalog call).
    sc_data: Mutex<HashMap<Platform, Box<LoadedScData>>>,
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

    /// Load (if needed) the SC data for the highest-priority install,
    /// returning a guard that holds the lock for the duration of the
    /// caller's read. In Stage 2.5 only one platform is loaded eagerly;
    /// Stage 3+ adds a channel switcher.
    async fn loaded(&self) -> Result<LoadedRef<'_>, AppError> {
        let mut guard = self.sc_data.lock().await;
        if guard.is_empty() {
            let loaded = LoadedScData::load_async()
                .await
                .map_err(|e| AppError::NoInstall(format!("{e:#}")))?;
            guard.insert(loaded.platform, loaded);
        }
        Ok(LoadedRef { guard })
    }

    /// Resolve the currently-active account, bootstrapping a row from
    /// the launcher store's handle if needed. Returns the live `Account`
    /// row.
    ///
    /// Bootstrap rule: at first run the launcher store's `nickname` is
    /// the active account. If no `accounts` row matches, insert one.
    /// Stage 3+ adds a multi-account picker on top of this.
    async fn active_account(&self) -> Result<Account, AppError> {
        let handle = {
            let loaded = self.loaded().await?;
            loaded
                .data()
                .handle
                .clone()
                .ok_or_else(|| AppError::Internal(
                    "no RSI handle available — launcher store identity could not be read"
                        .into(),
                ))?
        };
        let db = self.db().await?;
        hearth_storage::upsert_account_by_handle(db, &handle)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))
    }

    async fn active_scope(&self) -> Result<Scope, AppError> {
        let platform = {
            let loaded = self.loaded().await?;
            loaded.data().platform
        };
        let account = self.active_account().await?;
        Ok(Scope::new(platform, account.id))
    }
}

/// RAII handle to one platform's loaded data. Drops the lock when out
/// of scope.
struct LoadedRef<'a> {
    guard: tokio::sync::MutexGuard<'a, HashMap<Platform, Box<LoadedScData>>>,
}

impl LoadedRef<'_> {
    fn data(&self) -> &LoadedScData {
        // Stage 2.5: only one platform loaded at a time. Stage 3+
        // stores an explicit "active platform" alongside the map.
        self.guard.values().next().expect("loaded data present")
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
async fn list_blueprints(state: tauri::State<'_, AppState>) -> Result<Vec<BpView>, AppError> {
    let loaded = state.loaded().await?;
    Ok(loaded.data().blueprints())
}

#[tauri::command]
#[specta::specta]
async fn list_owned(state: tauri::State<'_, AppState>) -> Result<Vec<OwnedBlueprint>, AppError> {
    let scope = state.active_scope().await?;
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
    let scope = state.active_scope().await?;
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
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::remove_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Flip ownership of a blueprint in the active scope. Returns the new
/// owned state (`true` = now owned). Stage 3's primary write path.
#[tauri::command]
#[specta::specta]
async fn toggle_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    let currently_owned = hearth_storage::get_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .is_some();
    if currently_owned {
        hearth_storage::remove_owned(db, scope, &blueprint_guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        Ok(false)
    } else {
        hearth_storage::add_owned(db, scope, &blueprint_guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        Ok(true)
    }
}

#[tauri::command]
#[specta::specta]
async fn list_wishlist(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<WishlistEntry>, AppError> {
    let scope = state.active_scope().await?;
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
    let scope = state.active_scope().await?;
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
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::remove_from_wishlist(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Surface the active platform + channel + account so the UI can show
/// "PU · LIVE · @VeeLume" or similar. Triggers the bootstrap if needed
/// (loads SC data + inserts the account row if missing).
#[tauri::command]
#[specta::specta]
async fn active_scope(state: tauri::State<'_, AppState>) -> Result<ActiveScope, AppError> {
    let (platform, channel) = {
        let loaded = state.loaded().await?;
        let d = loaded.data();
        (d.platform, d.channel.display_name().to_string())
    };
    let account = state.active_account().await?;
    Ok(ActiveScope {
        platform,
        channel,
        account,
    })
}

/// List every RSI account this desktop has known. Stage 3 wires this
/// to a picker; Stage 2.5 just exposes the data.
#[tauri::command]
#[specta::specta]
async fn list_accounts(state: tauri::State<'_, AppState>) -> Result<Vec<Account>, AppError> {
    let db = state.db().await?;
    hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Scrape `/citizens/<handle>` for the given account and write the
/// immutable anchors (`citizen_record`, `enlisted`) back to the row.
/// Refreshes `last_verified`. Returns the up-to-date `Account`.
#[tauri::command]
#[specta::specta]
async fn verify_account(
    state: tauri::State<'_, AppState>,
    account_id: RecordId,
) -> Result<Account, AppError> {
    let db = state.db().await?;
    let account = hearth_storage::get_account(db, account_id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .ok_or_else(|| AppError::Internal(format!("account {account_id} not found")))?;
    let info = identity::fetch_profile(&account.handle)
        .await
        .map_err(|e| AppError::Identity(format!("{e:#}")))?;
    hearth_storage::update_account_anchors(db, account.id, info.citizen_record, &info.enlisted)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    hearth_storage::get_account(db, account.id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .ok_or_else(|| AppError::Internal("account vanished after update".into()))
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct ActiveScope {
    platform: Platform,
    channel: String,
    account: Account,
}

// ── App setup ───────────────────────────────────────────────────────────────

/// Single source of truth for the IPC command list. Used both by
/// `run()` at app startup and by the `export-bindings` binary so the
/// TypeScript file can be regenerated without booting the full Tauri
/// app (which would require loading SC data).
pub fn ipc_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        list_blueprints,
        list_owned,
        add_owned,
        remove_owned,
        toggle_owned,
        list_wishlist,
        add_to_wishlist,
        remove_from_wishlist,
        active_scope,
        list_accounts,
        verify_account,
    ])
}

/// Write `src/lib/bindings.ts` from the current Rust command surface.
/// Idempotent. Called from `run()` debug builds and from the
/// `export-bindings` binary.
pub fn export_bindings(out: &str) -> Result<(), specta_typescript::ExportError> {
    ipc_builder().export(typescript_exporter(), out)
}

/// Shared TS exporter config. `BigInt → Number` is safe for our small
/// i64 fields (citizen records ~7 digits, heapAccountId ~7 digits).
fn typescript_exporter() -> Typescript {
    Typescript::default().bigint(BigIntExportBehavior::Number)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        sc_data: Mutex::new(HashMap::new()),
        db: OnceCell::new(),
    };

    let builder = ipc_builder();

    #[cfg(debug_assertions)]
    builder
        .export(typescript_exporter(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
