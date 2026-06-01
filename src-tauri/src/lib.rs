//! Tauri shell for Hearth.
//!
//! # Startup shape
//!
//! AppState splits the SC-load along the fast/slow seam, with each piece
//! in its own OnceCell so concurrent readers don't serialize on a mutex
//! and commands wait only on the data they actually need:
//!
//! - [`AppState::discovery`] — ~50ms. Install + handle + platform.
//!   Required by the sidebar scope chip and all DB-only commands
//!   (`list_owned`, `toggle_owned`, `active_scope`, …).
//! - [`AppState::catalog`] — 0.15s warm / ~30s cold. The cooked
//!   `Vec<BpView>`. Required only by `list_blueprints`.
//! - [`AppState::db`] — independent SQLite pool, initialized on first
//!   DB-touching command on Tauri's tokio runtime.
//!
//! `setup()` eagerly fires discovery + catalog + db on a background task
//! so the OnceCells are warm by the time the WebView mounts and starts
//! firing onMount IPC calls. Cold paths still pay their cost, but the
//! UI can show identity and accept clicks while the catalog builds in
//! the background.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use hearth_core::{
    Account, BpView, MissionRef, MissionView, OwnedBlueprint, Platform, RecordId, WishIntent,
    WishlistEntry,
};
use hearth_storage::{DbPool, Scope};
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{Emitter, Manager};
use tauri_specta::{Builder, collect_commands};
use tokio::sync::OnceCell;

pub mod error;
pub mod export;
pub mod identity;
pub mod sc_loader;
pub mod sensors;

use error::AppError;
use sc_loader::Discovery;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    /// Fast install/handle bundle. ~50ms first call; lock-free after.
    /// Required for the sidebar scope chip and every DB-scoped command
    /// (they need the platform + active account, both derived from
    /// this).
    discovery: OnceCell<Discovery>,
    /// Cooked SC reference data (blueprint catalog + missions). Loaded
    /// lazily via the snapshot waterfall in `sc_loader::build_data`. Only
    /// `list_blueprints` / `list_missions` await this; other commands stay
    /// fast.
    data: OnceCell<sc_loader::CookedData>,
    /// SQLite pool, lazily initialized on first DB-needing command.
    db: OnceCell<DbPool>,
}

impl AppState {
    fn new() -> Self {
        Self {
            discovery: OnceCell::new(),
            data: OnceCell::new(),
            db: OnceCell::new(),
        }
    }

    /// Get the fast discovery bundle (install + handle + platform).
    /// Initialized once on first call; subsequent calls are lock-free.
    async fn discovery(&self) -> Result<&Discovery, AppError> {
        self.discovery
            .get_or_try_init(|| async {
                sc_loader::discover()
                    .await
                    .map_err(|e| AppError::NoInstall(format!("{e:#}")))
            })
            .await
    }

    /// Get the cooked SC reference data (catalog + missions). Awaits
    /// `discovery()` first to know which install to parse, then runs the
    /// snapshot waterfall on first call. Both products share one parse, so
    /// warming this once serves `list_blueprints` and `list_missions`.
    async fn data(&self) -> Result<&sc_loader::CookedData, AppError> {
        // Pull the install out before initializing so the discovery borrow
        // doesn't span the data init.
        let install = {
            let d = self.discovery().await?;
            d.install.clone()
        };
        self.data
            .get_or_try_init(|| async move {
                sc_loader::build_data(install)
                    .await
                    .map_err(|e| AppError::Internal(format!("{e:#}")))
            })
            .await
    }

    /// The cooked blueprint catalog (projection of [`Self::data`]).
    async fn catalog(&self) -> Result<&Vec<BpView>, AppError> {
        Ok(&self.data().await?.blueprints)
    }

    /// The cooked mission browser data (projection of [`Self::data`]).
    async fn missions(&self) -> Result<&Vec<MissionView>, AppError> {
        Ok(&self.data().await?.missions)
    }

    async fn db(&self) -> Result<&DbPool, AppError> {
        self.db
            .get_or_try_init(|| async {
                hearth_storage::open(&db_path())
                    .await
                    .map_err(|e| AppError::Storage(format!("{e:#}")))
            })
            .await
    }

    /// Resolve the currently-active account, bootstrapping a row from
    /// the launcher store's handle if needed. Returns the live `Account`
    /// row. Fast — needs only discovery + db, not the catalog.
    ///
    /// Bootstrap rule: at first run the launcher store's `nickname` is
    /// the active account. If no `accounts` row matches, insert one.
    /// Stage 3+ adds a multi-account picker on top of this.
    async fn active_account(&self) -> Result<Account, AppError> {
        let handle = self
            .discovery()
            .await?
            .handle
            .clone()
            .ok_or_else(|| {
                AppError::Internal(
                    "no RSI handle available — launcher store identity could not be read"
                        .into(),
                )
            })?;
        let db = self.db().await?;
        hearth_storage::upsert_account_by_handle(db, &handle)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))
    }

    async fn active_scope(&self) -> Result<Scope, AppError> {
        let platform = self.discovery().await?.platform;
        let account = self.active_account().await?;
        Ok(Scope::new(platform, account.id))
    }

    /// Rewrite the sc-langpatch owned-blueprints export (Stage 4) from the
    /// active scope's owned set. Best-effort: a failure is logged, never
    /// surfaced, so it can't break the ownership toggle that triggered it —
    /// the file is regenerated on the next change. Called after every
    /// ownership mutation and once at warmup so langpatch's startup read
    /// finds a current file.
    async fn refresh_owned_export(&self) {
        if let Err(e) = self.try_refresh_owned_export().await {
            tracing::warn!("owned-blueprints export skipped: {e:#}");
        }
    }

    async fn try_refresh_owned_export(&self) -> Result<(), AppError> {
        let scope = self.active_scope().await?;
        let db = self.db().await?;
        let guids: Vec<String> = hearth_storage::list_owned(db, scope)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?
            .into_iter()
            .map(|o| o.blueprint_guid)
            .collect();
        // FS work off the async executor.
        tokio::task::spawn_blocking(move || export::write_owned(&guids))
            .await
            .map_err(|e| AppError::Internal(format!("export join: {e}")))?
            .map_err(|e| AppError::Internal(format!("{e:#}")))
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
    Ok(state.catalog().await?.clone())
}

#[tauri::command]
#[specta::specta]
async fn list_missions(state: tauri::State<'_, AppState>) -> Result<Vec<MissionView>, AppError> {
    Ok(state.missions().await?.clone())
}

/// Map of `blueprint_record_guid → missions that grant it`, derived by
/// inverting the cooked mission list. Powers the wishlist's ⚐ fulfilment slot
/// ("which missions grant this blueprint?"). Awaits the same shared cooked
/// data as `list_missions` — no extra load cost when the catalog is warm.
#[tauri::command]
#[specta::specta]
async fn missions_by_blueprint(
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, Vec<MissionRef>>, AppError> {
    Ok(hearth_core::missions_by_blueprint(state.missions().await?))
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
    let added = hearth_storage::add_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    state.refresh_owned_export().await;
    Ok(added)
}

#[tauri::command]
#[specta::specta]
async fn remove_owned(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    let removed = hearth_storage::remove_owned(db, scope, &blueprint_guid)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    state.refresh_owned_export().await;
    Ok(removed)
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
    let now_owned = if currently_owned {
        hearth_storage::remove_owned(db, scope, &blueprint_guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        false
    } else {
        hearth_storage::add_owned(db, scope, &blueprint_guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        true
    };
    state.refresh_owned_export().await;
    Ok(now_owned)
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

/// Flip one wishlist intent for a blueprint in the active scope. The two
/// intents (`Recipe` = want the BP, `Item` = want a crafted copy) toggle
/// independently. Returns the new state (`true` = now wanted).
#[tauri::command]
#[specta::specta]
async fn toggle_wishlist(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
    intent: WishIntent,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    let currently_wanted = hearth_storage::get_wishlist_entry(db, scope, &blueprint_guid, intent)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .is_some();
    if currently_wanted {
        hearth_storage::remove_from_wishlist(db, scope, &blueprint_guid, intent)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        Ok(false)
    } else {
        hearth_storage::add_to_wishlist(db, scope, &blueprint_guid, intent)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        Ok(true)
    }
}

/// Surface the active platform + channel + account so the UI can show
/// "PU · LIVE · @VeeLume" or similar. Fast — needs only discovery + db,
/// not the catalog, so the sidebar renders without waiting on the DCB
/// parse.
#[tauri::command]
#[specta::specta]
async fn active_scope(state: tauri::State<'_, AppState>) -> Result<ActiveScope, AppError> {
    let (platform, channel) = {
        let d = state.discovery().await?;
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

// ── Debug commands ──────────────────────────────────────────────────────────

/// Wipe the SC reference-data snapshot cache at
/// `%APPDATA%/hearth/cache/` (every channel's `catalog.cook` +
/// `extract.snap`) and restart the app so the OnceCells in AppState
/// reload from scratch.
///
/// Personal-state data (the `hearth.db` SQLite) is *not* touched —
/// owned blueprints, accounts, and the rest of `%APPDATA%/hearth/`
/// outside `cache/` stays put. After the restart, the next launch
/// runs the cold-path live parse (~30s on a typical install) before
/// catalog UI becomes responsive again.
///
/// Diverges via `AppHandle::restart()` — never returns to the caller
/// on success. The frontend should expect either an error reply or a
/// hard restart.
#[tauri::command]
#[specta::specta]
async fn wipe_sc_cache(app: tauri::AppHandle) -> Result<(), AppError> {
    let cache_root = dirs::data_dir()
        .map(|d| d.join("hearth").join("cache"))
        .ok_or_else(|| AppError::Internal("no platform data dir".into()))?;
    if cache_root.exists() {
        std::fs::remove_dir_all(&cache_root).map_err(|e| {
            AppError::Internal(format!(
                "removing cache dir {}: {e}",
                cache_root.display()
            ))
        })?;
        tracing::info!(path = %cache_root.display(), "wiped SC snapshot cache");
    } else {
        tracing::info!("no SC cache dir present; nothing to wipe");
    }
    // `restart()` returns `!` — the process is replaced before the
    // future ever resolves, so the Result `Ok` is never reached.
    app.restart()
}

// ── App setup ───────────────────────────────────────────────────────────────

/// Single source of truth for the IPC command list. Used both by
/// `run()` at app startup and by the `export-bindings` binary so the
/// TypeScript file can be regenerated without booting the full Tauri
/// app (which would require loading SC data).
pub fn ipc_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        list_blueprints,
        list_missions,
        missions_by_blueprint,
        list_owned,
        add_owned,
        remove_owned,
        toggle_owned,
        list_wishlist,
        toggle_wishlist,
        active_scope,
        list_accounts,
        verify_account,
        wipe_sc_cache,
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

/// Spawn the eager warmup: discovery, catalog, db all start filling
/// their OnceCells in parallel. By the time the WebView hydrates and
/// onMount fires (~1-4s later), the cells are likely already populated
/// and the IPC calls return instantly. Failures here are silent —
/// callers will hit the same errors on demand and report them through
/// AppError.
fn spawn_warmup(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        // Discovery first because catalog depends on it. Catalog + db
        // can then run concurrently.
        if state.discovery().await.is_ok() {
            let _ = tokio::join!(state.catalog(), state.db());
            // Seed the langpatch export so a current file exists before the
            // first ownership toggle (best-effort; logs on failure).
            state.refresh_owned_export().await;
        } else {
            // No install: at least try the DB so personal-state queries
            // get a clean DB error instead of a slow no-pool wait.
            let _ = state.db().await;
        }
    });
}

/// Payload of the `blueprints-sensed` event — emitted after a Game.log poll
/// that auto-marked (or failed to resolve) blueprints, so the UI can toast
/// "Hearth marked N owned" and refresh.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct BlueprintsSensed {
    /// Display names that resolved to ≥1 catalog blueprint this poll.
    marked: Vec<String>,
    /// `blueprint_record_guid`s newly flipped to owned (skips already-owned).
    newly_owned: Vec<String>,
    /// Sensed names that matched no catalog blueprint (name drift / locale).
    unresolved: Vec<String>,
}

/// Catalog display-name → `blueprint_record_guid`s. One name can map to
/// several interchangeable BPs (variants / duplicate-BP collapse); the
/// sensor marks all of them, consistent with entity-level ownership.
fn build_name_index(catalog: &[BpView]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for bp in catalog {
        if let Some(name) = &bp.display_name {
            let key = name.trim().to_lowercase();
            if !key.is_empty() {
                index.entry(key).or_default().push(bp.blueprint_record_guid.clone());
            }
        }
    }
    index
}

/// v1.5 auto-sensing: tail the active install's `Game.log`, and when the
/// logged session matches the active account + platform (pollution guard),
/// auto-mark received blueprints owned and emit `blueprints-sensed`.
///
/// Best-effort throughout — no install, no handle, or a poll error just means
/// no sensing; nothing here can break the rest of the app.
fn spawn_sensor(handle: tauri::AppHandle) {
    const POLL: Duration = Duration::from_secs(4);

    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();

        // Needs the install (for the log path + the active handle/platform to
        // guard against) and the catalog (for name → guid resolution).
        let (log_path, active_platform, active_handle) = match state.discovery().await {
            Ok(d) => (
                sensors::game_log_path(&d.install.root),
                d.platform,
                d.handle.clone(),
            ),
            Err(_) => return, // no install → nothing to sense
        };
        let name_index = match state.catalog().await {
            Ok(catalog) => build_name_index(catalog),
            Err(_) => return,
        };
        if active_handle.is_none() {
            // Without the launcher handle we can't pollution-guard (and owned
            // writes would fail anyway — they need an active account). Don't
            // tail; the user can still mark BPs manually.
            tracing::info!("sensor disabled: no launcher handle to guard against");
            return;
        }

        tracing::info!(path = %log_path.display(), "Game.log sensor started");
        let mut tailer = sensors::GameLogTailer::new(log_path);
        // Session header carried across polls (the handle/platform are logged
        // once near the top; the first poll backfills the whole file).
        let mut sensed_platform: Option<Platform> = None;
        let mut sensed_handle: Option<String> = None;
        let mut ticker = tokio::time::interval(POLL);

        loop {
            ticker.tick().await;
            let events = match tailer.poll() {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Game.log poll failed: {e}");
                    continue;
                }
            };
            if events.is_empty() {
                continue;
            }

            // First pass: fold session state + collect guarded blueprint hits.
            let mut to_mark: Vec<(String, Vec<String>)> = Vec::new(); // (name, guids)
            let mut unresolved: Vec<String> = Vec::new();
            for ev in events {
                match ev {
                    sensors::SensedEvent::SessionPlatform(p) => sensed_platform = Some(p),
                    sensors::SensedEvent::SessionHandle(h) => sensed_handle = Some(h),
                    // accountId isn't part of the live guard (the live session
                    // is always the active account by definition); it's used by
                    // the history import to group renamed-account sessions.
                    sensors::SensedEvent::SessionAccountId(_) => {}
                    sensors::SensedEvent::BlueprintReceived { name } => {
                        // Pollution guard: same platform AND same handle as
                        // the active account, else this log isn't ours to act on.
                        let guard_ok = sensed_platform == Some(active_platform)
                            && matches!(
                                (&sensed_handle, &active_handle),
                                (Some(s), Some(a)) if s.eq_ignore_ascii_case(a)
                            );
                        if !guard_ok {
                            tracing::debug!(
                                bp = %name,
                                "sensed blueprint skipped — session doesn't match active account/platform"
                            );
                            continue;
                        }
                        match name_index.get(&name.trim().to_lowercase()) {
                            Some(guids) => to_mark.push((name, guids.clone())),
                            None => unresolved.push(name),
                        }
                    }
                }
            }

            if to_mark.is_empty() && unresolved.is_empty() {
                continue;
            }

            // Second pass: mark owned (resolve scope + db once for the batch).
            let mut marked = Vec::new();
            let mut newly_owned = Vec::new();
            if !to_mark.is_empty() {
                match (state.active_scope().await, state.db().await) {
                    (Ok(scope), Ok(db)) => {
                        for (name, guids) in to_mark {
                            for guid in guids {
                                match hearth_storage::get_owned(db, scope, &guid).await {
                                    Ok(Some(_)) => {} // already owned
                                    Ok(None) => {
                                        match hearth_storage::add_owned(db, scope, &guid).await {
                                            Ok(_) => newly_owned.push(guid),
                                            Err(e) => tracing::warn!("sensor add_owned failed: {e:#}"),
                                        }
                                    }
                                    Err(e) => tracing::warn!("sensor get_owned failed: {e:#}"),
                                }
                            }
                            marked.push(name);
                        }
                    }
                    _ => {
                        tracing::warn!("sensor could not resolve scope/db; skipping this batch");
                        continue;
                    }
                }
            }

            if !newly_owned.is_empty() {
                state.refresh_owned_export().await; // keep the langpatch export in sync
            }
            tracing::info!(
                marked = marked.len(),
                newly_owned = newly_owned.len(),
                unresolved = unresolved.len(),
                "Game.log sensing pass"
            );
            if let Err(e) = handle.emit(
                "blueprints-sensed",
                BlueprintsSensed { marked, newly_owned, unresolved },
            ) {
                tracing::warn!("failed to emit blueprints-sensed: {e}");
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = ipc_builder();

    #[cfg(debug_assertions)]
    builder
        .export(typescript_exporter(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            spawn_warmup(app.handle().clone());
            spawn_sensor(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("running tauri application");
}
