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
pub mod notify;
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
    /// Cached result of the last `scan_log_history` so `apply_log_import`
    /// doesn't re-read the ~900 backup logs. Cleared after a successful apply.
    import_scan: std::sync::Mutex<Vec<ScannedIdentity>>,
    /// Resolved active RSI handle, cached once. The launcher store only persists
    /// the identity when "Remember Me" is checked; this falls back to the live
    /// `Game.log`, the last-active handle, or a sole known account so the
    /// account-scoped app still works. See [`AppState::active_handle`].
    resolved_handle: OnceCell<String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            discovery: OnceCell::new(),
            data: OnceCell::new(),
            db: OnceCell::new(),
            import_scan: std::sync::Mutex::new(Vec::new()),
            resolved_handle: OnceCell::new(),
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

    /// The active RSI handle, resolved once and cached. The launcher store only
    /// persists the identity when the user checked "Remember Me", so a raw
    /// `discovery().handle` is frequently `None` — which used to break every
    /// account-scoped command. This falls back through the live `Game.log`, the
    /// last handle we ran against, and a sole known account before giving up.
    ///
    /// Cached in a `OnceCell`: the active account is fixed per session anyway
    /// (`discovery` itself is cached; switching accounts means a restart), and
    /// caching keeps the Game.log read off the hot path. A failure isn't cached,
    /// so a later call (after the game has written its log) can still succeed.
    async fn active_handle(&self) -> Result<String, AppError> {
        self.resolved_handle
            .get_or_try_init(|| self.resolve_handle())
            .await
            .cloned()
    }

    async fn resolve_handle(&self) -> Result<String, AppError> {
        let d = self.discovery().await?;
        // 1. Launcher store identity (present only with "Remember Me").
        if let Some(h) = d.handle.clone() {
            return Ok(h);
        }
        // 2. The live Game.log's logged-in handle — written every session
        //    regardless of "Remember Me", so it reflects who is actually
        //    playing right now (or who played last).
        let log_path = sensors::game_log_path(&d.install.root);
        if let Some(h) =
            tokio::task::spawn_blocking(move || read_game_log_handle(&log_path))
                .await
                .map_err(|e| AppError::Internal(format!("game-log read join: {e}")))?
        {
            tracing::info!(handle = %h, "active handle resolved from Game.log (launcher identity unavailable)");
            return Ok(h);
        }
        // 3. The last handle we ran against, if it still maps to an account.
        let db = self.db().await?;
        if let Some(last) = hearth_storage::get_setting(db, LAST_ACTIVE_HANDLE)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?
            && hearth_storage::account_id_for_handle(db, &last)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?
                .is_some()
        {
            tracing::info!(handle = %last, "active handle resolved from last-active setting");
            return Ok(last);
        }
        // 4. A single known account is unambiguous.
        let accounts = hearth_storage::list_accounts(db)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        if let [only] = accounts.as_slice() {
            tracing::info!(handle = %only.handle, "active handle resolved to the sole known account");
            return Ok(only.handle.clone());
        }
        Err(AppError::Internal(
            "no RSI identity available — check \"Remember Me\" in the RSI launcher, \
             or sign in and launch the game once so Hearth can read it"
                .into(),
        ))
    }

    /// Resolve the currently-active account, bootstrapping a row from the
    /// resolved handle if needed. Returns the live `Account` row. Fast — needs
    /// only discovery + db (+ a one-time Game.log read in the fallback path),
    /// not the catalog.
    async fn active_account(&self) -> Result<Account, AppError> {
        let handle = self.active_handle().await?;
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

/// Root of Hearth's on-disk data (DB, SC cache, langpatch export) under the
/// OS data dir.
///
/// **Dev / release isolation:** debug builds (`cargo tauri dev`) use a separate
/// `hearth-dev` namespace, so iterating on the dev build — deleting the DB on a
/// schema change, wiping the SC cache — never touches real release data. The
/// installed release binary uses `hearth`. `HEARTH_DATA_DIR` overrides both:
/// an escape hatch to point a dev build at release data, or to spin up a
/// throwaway profile.
pub(crate) fn app_data_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("HEARTH_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let namespace = if cfg!(debug_assertions) { "hearth-dev" } else { "hearth" };
    dirs::data_dir()
        .map(|d| d.join(namespace))
        .expect("OS data dir not resolvable")
}

/// `<app_data_root>/hearth.db`.
fn db_path() -> PathBuf {
    app_data_root().join("hearth.db")
}

// ── Account identity / log-history import ─────────────────────────────────────

/// One RSI identity discovered across the session logs — a `(account_hint
/// else handle)` group. Cached between `scan_log_history` and
/// `apply_log_import` so the heavy log read happens once. Internal (not over
/// the IPC boundary); the UI sees the leaner [`DiscoveredIdentity`].
#[derive(Debug, Clone)]
struct ScannedIdentity {
    /// Stable grouping key echoed back by the UI in an [`ImportChoice`].
    key: String,
    account_hint: Option<i64>,
    /// Distinct handles seen for this identity (rename history), first-seen order.
    handles: Vec<String>,
    /// Distinct blueprint display names received across the grouped sessions.
    blueprint_names: Vec<String>,
    session_count: u32,
}

/// An account plus its recorded past handles — the Accounts UI shape.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct AccountWithAliases {
    account: Account,
    aliases: Vec<String>,
}

/// A discovered identity surfaced to the UI for classification.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct DiscoveredIdentity {
    key: String,
    account_hint: Option<i64>,
    handles: Vec<String>,
    session_count: u32,
    blueprint_count: u32,
    /// Best-guess existing account this maps to (handle/alias/hint match).
    suggested_account_id: Option<RecordId>,
    /// That account's current handle, for display.
    suggested_handle: Option<String>,
}

/// The UI's decision for one discovered identity.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
struct ImportChoice {
    key: String,
    /// `"existing"` (use `account_id`), `"new"` (create from the identity's
    /// primary handle), or `"ignore"`.
    action: String,
    account_id: Option<RecordId>,
}

/// Summary of an applied import.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct ImportResult {
    accounts_touched: u32,
    newly_owned: u32,
    /// Blueprint display names that matched no catalog entry (name drift).
    unresolved: Vec<String>,
}

/// All session-log files for a channel: the live `Game.log` + every
/// `logbackups/*.log`.
fn session_log_files(channel_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let live = sensors::game_log_path(channel_dir);
    if live.exists() {
        files.push(live);
    }
    if let Ok(entries) = std::fs::read_dir(sensors::log_backups_dir(channel_dir)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("log") {
                files.push(path);
            }
        }
    }
    files
}

/// One cached per-file scan: the file's mtime + length (the cache key) and the
/// summary it produced. `logbackups/` files are immutable once rotated, so a
/// matching mtime+len means the cached summary is still valid.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedScan {
    mtime: i64,
    len: u64,
    summary: sensors::SessionSummary,
}

/// Per-file scan cache (`<file path> → cached summary`). Lets a re-scan reuse
/// every unchanged backup and only parse new ones — so the first import pays the
/// full cost once; later runs are near-instant. Regenerable; a decode failure
/// just falls back to a full scan.
fn import_cache_path() -> PathBuf {
    app_data_root().join("import-scan-cache.json")
}

fn load_scan_cache() -> HashMap<String, CachedScan> {
    std::fs::read(import_cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_scan_cache(cache: &HashMap<String, CachedScan>) {
    let path = import_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(cache) {
        let _ = std::fs::write(&path, bytes); // best-effort
    }
}

fn file_mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Read + summarise every session log (blocking — hundreds of files). Parses
/// files in parallel across cores, reusing cached summaries for unchanged
/// backups; the live `Game.log` is always re-parsed (and never cached). Rebuilds
/// + persists the cache from this scan, which prunes deleted backups.
fn summarize_all_sessions(channel_dir: &std::path::Path) -> Vec<sensors::SessionSummary> {
    use rayon::prelude::*;

    let files = session_log_files(channel_dir);
    let live = sensors::game_log_path(channel_dir);
    let cache = load_scan_cache();

    // Per file: (summary, optional (key, entry) to persist). `None` entry = the
    // live Game.log (don't cache). A cache hit re-emits the existing entry.
    let scanned: Vec<(sensors::SessionSummary, Option<(String, CachedScan)>)> = files
        .par_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            let len = meta.len();
            let mtime = file_mtime_secs(&meta);
            let key = path.to_string_lossy().into_owned();
            let is_live = path.as_path() == live.as_path();

            if !is_live
                && let Some(c) = cache.get(&key)
                && c.mtime == mtime
                && c.len == len
            {
                // Hit — reuse the cached summary, keep caching it.
                let summary = c.summary.clone();
                return Some((summary.clone(), Some((key, CachedScan { mtime, len, summary }))));
            }

            let file = std::fs::File::open(path).ok()?;
            let summary = sensors::summarize_session(std::io::BufReader::new(file));
            let entry = (!is_live).then(|| (key, CachedScan { mtime, len, summary: summary.clone() }));
            Some((summary, entry))
        })
        .collect();

    let mut new_cache = HashMap::new();
    let mut summaries = Vec::with_capacity(scanned.len());
    for (summary, entry) in scanned {
        if let Some((key, c)) = entry {
            new_cache.insert(key, c);
        }
        summaries.push(summary);
    }
    save_scan_cache(&new_cache);
    summaries
}

/// Group prod-only session summaries into identities, keyed by numeric
/// `accountId` when present (so renames fold together) else by handle. PTU /
/// test-shard sessions are excluded — those scopes wipe, so importing their
/// history is pointless.
fn group_identities(summaries: Vec<sensors::SessionSummary>) -> Vec<ScannedIdentity> {
    let mut groups: HashMap<String, ScannedIdentity> = HashMap::new();
    for s in summaries {
        if s.platform != Some(Platform::Prod) {
            continue;
        }
        let key = match (s.account_hint, s.handle.as_deref()) {
            (Some(hint), _) => format!("hint:{hint}"),
            (None, Some(handle)) => format!("handle:{}", handle.to_lowercase()),
            (None, None) => continue, // anonymous session — nothing to attribute
        };
        let group = groups.entry(key.clone()).or_insert_with(|| ScannedIdentity {
            key,
            account_hint: s.account_hint,
            handles: Vec::new(),
            blueprint_names: Vec::new(),
            session_count: 0,
        });
        group.session_count += 1;
        if group.account_hint.is_none() {
            group.account_hint = s.account_hint;
        }
        if let Some(handle) = &s.handle
            && !group.handles.iter().any(|h| h.eq_ignore_ascii_case(handle))
        {
            group.handles.push(handle.clone());
        }
        for name in s.blueprint_names {
            if !group.blueprint_names.contains(&name) {
                group.blueprint_names.push(name);
            }
        }
    }
    let mut out: Vec<ScannedIdentity> = groups.into_values().collect();
    // Most-played identities first.
    out.sort_by(|a, b| b.session_count.cmp(&a.session_count).then(a.key.cmp(&b.key)));
    out
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

// ── Account management + log-history import ───────────────────────────────────

/// Accounts with their recorded past handles, for the Accounts UI.
#[tauri::command]
#[specta::specta]
async fn list_accounts_detailed(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    let accounts = hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    let mut out = Vec::with_capacity(accounts.len());
    for account in accounts {
        let aliases = hearth_storage::list_account_aliases(db, account.id)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        out.push(AccountWithAliases { account, aliases });
    }
    Ok(out)
}

/// Manually record a past handle for an account (rename the model didn't catch).
#[tauri::command]
#[specta::specta]
async fn add_account_alias(
    state: tauri::State<'_, AppState>,
    account_id: RecordId,
    handle: String,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    hearth_storage::add_account_alias(db, account_id, &handle)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    list_accounts_detailed(state).await
}

/// Merge one account into another (same person, two rows — e.g. a rename that
/// created a duplicate). Reassigns owned + wishlist data; `from` is absorbed.
/// Manual + explicit: the tool never auto-merges two accounts.
#[tauri::command]
#[specta::specta]
async fn merge_accounts(
    state: tauri::State<'_, AppState>,
    from: RecordId,
    into: RecordId,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    hearth_storage::merge_accounts(db, from, into)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    // The active account's owned set may have changed — keep the export fresh.
    state.refresh_owned_export().await;
    list_accounts_detailed(state).await
}

/// Scan the install's session logs (live + `logbackups/`) and surface the RSI
/// identities found, grouped by numeric `accountId` (renames fold together).
/// Caches the full scan in-state for [`apply_log_import`].
#[tauri::command]
#[specta::specta]
async fn scan_log_history(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredIdentity>, AppError> {
    let channel_dir = state.discovery().await?.install.root.clone();
    let summaries = tokio::task::spawn_blocking(move || summarize_all_sessions(&channel_dir))
        .await
        .map_err(|e| AppError::Internal(format!("log scan join: {e}")))?;
    let identities = group_identities(summaries);

    // Build the UI view with a suggested existing-account mapping.
    let db = state.db().await?;
    let accounts = hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    let mut discovered = Vec::with_capacity(identities.len());
    for id in &identities {
        // Suggest by handle/alias first, then by matching numeric hint.
        let mut suggested = None;
        for handle in &id.handles {
            if let Some(account_id) = hearth_storage::account_id_for_handle(db, handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?
            {
                suggested = Some(account_id);
                break;
            }
        }
        if suggested.is_none()
            && let Some(hint) = id.account_hint
        {
            suggested = accounts
                .iter()
                .find(|a| a.account_hint == Some(hint))
                .map(|a| a.id);
        }
        let suggested_handle = suggested
            .and_then(|sid| accounts.iter().find(|a| a.id == sid))
            .map(|a| a.handle.clone());
        discovered.push(DiscoveredIdentity {
            key: id.key.clone(),
            account_hint: id.account_hint,
            handles: id.handles.clone(),
            session_count: id.session_count,
            blueprint_count: id.blueprint_names.len() as u32,
            suggested_account_id: suggested,
            suggested_handle,
        });
    }

    *state.import_scan.lock().unwrap() = identities;
    Ok(discovered)
}

/// Apply the user's classification of the scanned identities: create / alias
/// accounts and mark their blueprints owned (prod scope). Uses the cached scan
/// from [`scan_log_history`].
#[tauri::command]
#[specta::specta]
async fn apply_log_import(
    state: tauri::State<'_, AppState>,
    choices: Vec<ImportChoice>,
) -> Result<ImportResult, AppError> {
    let scan = state.import_scan.lock().unwrap().clone();
    if scan.is_empty() {
        return Err(AppError::Internal(
            "no scan to apply — run scan_log_history first".into(),
        ));
    }
    let name_index = build_name_index(state.catalog().await?);
    let db = state.db().await?;

    let mut accounts_touched = 0u32;
    let mut newly_owned = 0u32;
    let mut unresolved: Vec<String> = Vec::new();

    for choice in choices {
        let Some(identity) = scan.iter().find(|i| i.key == choice.key) else {
            continue;
        };
        // Resolve the target account per the choice.
        let account_id = match choice.action.as_str() {
            "ignore" => continue,
            "existing" => match choice.account_id {
                Some(id) => id,
                None => continue,
            },
            "new" => {
                let Some(primary) = identity.handles.first() else {
                    continue;
                };
                hearth_storage::create_account(db, primary, identity.account_hint)
                    .await
                    .map_err(|e| AppError::Storage(format!("{e:#}")))?
                    .id
            }
            _ => continue,
        };
        accounts_touched += 1;

        // Record every observed handle as an alias + carry the numeric hint.
        for handle in &identity.handles {
            hearth_storage::add_account_alias(db, account_id, handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        }
        if let Some(hint) = identity.account_hint {
            hearth_storage::set_account_hint(db, account_id, hint)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        }

        // Mark blueprints owned in the prod scope (history is prod-only).
        let scope = hearth_storage::Scope::new(Platform::Prod, account_id);
        for name in &identity.blueprint_names {
            match resolve_blueprint_guids(&name_index, name) {
                Some(guids) => {
                    for guid in guids {
                        let already = hearth_storage::get_owned(db, scope, guid)
                            .await
                            .map_err(|e| AppError::Storage(format!("{e:#}")))?
                            .is_some();
                        if !already {
                            hearth_storage::add_owned(db, scope, guid)
                                .await
                                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
                            newly_owned += 1;
                        }
                    }
                }
                None => {
                    if !unresolved.contains(name) {
                        unresolved.push(name.clone());
                    }
                }
            }
        }
    }

    state.import_scan.lock().unwrap().clear();
    state.refresh_owned_export().await;
    Ok(ImportResult { accounts_touched, newly_owned, unresolved })
}

/// Predict which cache tier the catalog load will use, so the loading screen
/// can name the path (and warn it may be slow). Fast — just checks snapshot
/// files on disk; needs only discovery.
#[tauri::command]
#[specta::specta]
async fn predicted_load_tier(
    state: tauri::State<'_, AppState>,
) -> Result<sc_loader::LoadTier, AppError> {
    let channel = state.discovery().await?.channel;
    Ok(sc_loader::predict_tier(channel))
}

// ── App settings + live blueprint sync ───────────────────────────────────────

const LIVE_SYNC_ENABLED: &str = "live_sync_enabled";
const LIVE_SYNC_CONSENTED: &str = "live_sync_consented";
const SENSOR_ENABLED: &str = "sensor_enabled";
const ONBOARDING_COMPLETED: &str = "onboarding_completed";
/// Last launcher handle we ran against — the steady-state guard for the startup
/// rename check (a network scrape only happens when this changes).
const LAST_ACTIVE_HANDLE: &str = "last_active_handle";

/// App-global preferences surfaced to the Settings page.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct AppSettings {
    /// Whether this build was compiled with the `live-sync` feature at all.
    live_sync_available: bool,
    /// Runtime toggle — off by default; the user opts in.
    live_sync_enabled: bool,
    /// Whether the one-time ToS consent has been acknowledged.
    live_sync_consented: bool,
    /// Live Game.log sensing (auto-mark BPs received during play). Default on.
    sensor_enabled: bool,
    /// Whether the first-launch onboarding has been completed/skipped.
    onboarding_completed: bool,
}

/// Outcome of one live sync, for the Settings UI + the notification body.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct LiveSyncResult {
    /// Blueprints the server reported owned.
    total: u32,
    /// Newly marked owned this sync.
    added: u32,
    /// Un-owned this sync (reconcile — the server no longer lists them).
    removed: u32,
    /// Server blueprint ids with no matching catalog entry.
    unresolved: u32,
}

async fn read_bool_setting(db: &DbPool, key: &str, default: bool) -> Result<bool, AppError> {
    Ok(hearth_storage::get_setting(db, key)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .map(|v| v == "true")
        .unwrap_or(default))
}

/// Build the current settings snapshot. One place so every setter returns a
/// consistent shape.
async fn read_settings(db: &DbPool) -> Result<AppSettings, AppError> {
    Ok(AppSettings {
        live_sync_available: cfg!(feature = "live-sync"),
        live_sync_enabled: read_bool_setting(db, LIVE_SYNC_ENABLED, false).await?,
        live_sync_consented: read_bool_setting(db, LIVE_SYNC_CONSENTED, false).await?,
        sensor_enabled: read_bool_setting(db, SENSOR_ENABLED, true).await?,
        onboarding_completed: read_bool_setting(db, ONBOARDING_COMPLETED, false).await?,
    })
}

#[tauri::command]
#[specta::specta]
async fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, AppError> {
    read_settings(state.db().await?).await
}

/// Enable/disable live blueprint sync. Enabling records consent — the UI shows
/// the one-time consent dialog before calling this with `enabled = true`.
#[tauri::command]
#[specta::specta]
async fn set_live_sync(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, LIVE_SYNC_ENABLED, if enabled { "true" } else { "false" })
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if enabled {
        hearth_storage::set_setting(db, LIVE_SYNC_CONSENTED, "true")
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    }
    read_settings(db).await
}

/// Enable/disable live Game.log sensing. Takes effect within one poll interval
/// (the sensor loop checks this each tick) — no restart needed.
#[tauri::command]
#[specta::specta]
async fn set_sensor(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, SENSOR_ENABLED, if enabled { "true" } else { "false" })
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    read_settings(db).await
}

/// Mark the first-launch onboarding as completed (or skipped) so it doesn't
/// show again. Re-running it from Settings doesn't clear this.
#[tauri::command]
#[specta::specta]
async fn set_onboarding_complete(
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, ONBOARDING_COMPLETED, "true")
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    read_settings(db).await
}

/// Fetch the authoritative owned-blueprint set from CIG's backend and reconcile
/// the active account's prod-scope owned set to it. Emits a success/error
/// notification regardless of caller.
#[tauri::command]
#[specta::specta]
async fn live_sync_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<LiveSyncResult, AppError> {
    #[cfg(feature = "live-sync")]
    {
        live_sync_impl(&app, state.inner()).await
    }
    #[cfg(not(feature = "live-sync"))]
    {
        let _ = (&app, &state);
        Err(AppError::LiveSync(
            "live blueprint sync is not available in this build".into(),
        ))
    }
}

/// Strip a guid to lowercase alphanumerics so server ids and catalog guids
/// compare regardless of dash/case formatting.
#[cfg(feature = "live-sync")]
fn norm_guid(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(feature = "live-sync")]
fn map_dossier_err(e: sc_dossier::Error) -> AppError {
    use sc_dossier::Error as E;
    let msg = match &e {
        E::Mint { .. } => {
            "Your RSI session looks stale — re-open the RSI launcher to refresh it, then sync again."
                .to_string()
        }
        E::StoreNotFound(_) | E::MissingCredential { .. } => {
            "No RSI launcher session found — open the RSI launcher and sign in.".to_string()
        }
        E::LauncherNotFound => "RSI Launcher not found on this machine.".to_string(),
        other => format!("{other}"),
    };
    AppError::LiveSync(msg)
}

/// The fetch + reconcile, without notification (the caller notifies).
#[cfg(feature = "live-sync")]
async fn live_sync_run(state: &AppState) -> Result<LiveSyncResult, AppError> {
    use std::collections::HashSet;
    const UA: &str = concat!("Hearth/", env!("CARGO_PKG_VERSION"));

    let dossier = sc_dossier::Dossier::from_launcher(UA)
        .await
        .map_err(map_dossier_err)?;
    let server = dossier.owned_blueprints().await.map_err(map_dossier_err)?;

    // Resolve server blueprint ids → catalog blueprint_record_guids (+ keep the
    // display name by normalized guid for diagnostics).
    let mut by_norm: HashMap<String, String> = HashMap::new();
    let mut name_by_norm: HashMap<String, Option<String>> = HashMap::new();
    for b in state.catalog().await?.iter() {
        let n = norm_guid(&b.blueprint_record_guid);
        name_by_norm.insert(n.clone(), b.display_name.clone());
        by_norm.insert(n, b.blueprint_record_guid.clone());
    }
    let mut target: HashSet<String> = HashSet::new();
    let mut unresolved: Vec<&sc_dossier::Blueprint> = Vec::new();
    for b in &server {
        match by_norm.get(&norm_guid(&b.blueprint_id)) {
            Some(guid) => {
                target.insert(guid.clone());
            }
            None => unresolved.push(b),
        }
    }
    // Diagnostic: name the server blueprints with no catalog match so we can
    // check what they are (e.g. a post-patch re-GUID, or outside the catalog's
    // Creation-process filter).
    for b in &unresolved {
        tracing::warn!(
            blueprint_id = %b.blueprint_id,
            item_class_id = %b.item_class_id,
            category_id = %b.category_id,
            "live sync: owned server blueprint not found in the catalog"
        );
    }

    // Reconcile the active account's prod-scope owned set to `target`. Prod
    // regardless of the launcher's current shard: the blueprint library is
    // account-level (PTU shards wipe), the same stance as the Game.log import.
    let account = state.active_account().await?;
    let scope = Scope::new(Platform::Prod, account.id);
    let db = state.db().await?;
    let current: HashSet<String> = hearth_storage::list_owned(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .into_iter()
        .map(|o| o.blueprint_guid)
        .collect();

    let mut added = 0u32;
    for guid in target.difference(&current) {
        hearth_storage::add_owned(db, scope, guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        added += 1;
    }
    let mut removed = 0u32;
    for guid in current.difference(&target) {
        if hearth_storage::remove_owned(db, scope, guid)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?
        {
            removed += 1;
        }
    }

    state.refresh_owned_export().await;

    // The server can return duplicate entries (gRPC pagination overlap), so the
    // raw count overstates ownership — report the distinct, in-catalog total.
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for b in &server {
        *counts.entry(b.blueprint_id.as_str()).or_default() += 1;
    }
    for (id, n) in counts.iter().filter(|&(_, &n)| n > 1) {
        let name = name_by_norm
            .get(&norm_guid(id))
            .and_then(|opt| opt.as_deref())
            .unwrap_or("?");
        tracing::warn!(blueprint_id = %id, name, count = n, "live sync: duplicate blueprint entry from server");
    }
    let distinct_ids = counts.len();
    tracing::info!(
        server_entries = server.len(),
        distinct_ids,
        owned = target.len(),
        added,
        removed,
        unresolved = unresolved.len(),
        "live sync reconcile"
    );

    Ok(LiveSyncResult {
        total: target.len() as u32,
        added,
        removed,
        unresolved: unresolved.len() as u32,
    })
}

/// Run a live sync and emit a success/error notification either way. Used by
/// both the `live_sync_now` command and the startup auto-sync.
#[cfg(feature = "live-sync")]
async fn live_sync_impl(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<LiveSyncResult, AppError> {
    match live_sync_run(state).await {
        Ok(r) => {
            emit_ownership_changed(app); // refresh the catalog's owned set
            let mut body = format!("{} added, {} removed", r.added, r.removed);
            if r.unresolved > 0 {
                body.push_str(&format!(" · {} not in catalog", r.unresolved));
            }
            notify::notify(
                app,
                notify::Notification::success(format!(
                    "Live sync: {} blueprint{} owned",
                    r.total,
                    plural(r.total as usize)
                ))
                .with_body(body)
                .with_action("View catalog", "/"),
            );
            Ok(r)
        }
        Err(e) => {
            notify::notify(
                app,
                notify::Notification::error("Live sync failed").with_body(e.to_string()),
            );
            Err(e)
        }
    }
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
    let cache_root = app_data_root().join("cache");
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
        list_accounts_detailed,
        add_account_alias,
        merge_accounts,
        scan_log_history,
        apply_log_import,
        predicted_load_tier,
        get_settings,
        set_live_sync,
        set_sensor,
        set_onboarding_complete,
        live_sync_now,
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
            let key = normalize_bp_name(name);
            if !key.is_empty() {
                index.entry(key).or_default().push(bp.blueprint_record_guid.clone());
            }
        }
    }
    index
}

fn normalize_bp_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// `""` for one, `"s"` for many — for pluralising notification copy.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A short, comma-joined preview of names for a notification body: the first
/// four, then `+N more`.
fn preview_names(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
    let mut s = shown.join(", ");
    if names.len() > 4 {
        s.push_str(&format!(", +{} more", names.len() - 4));
    }
    s
}

/// Resolve a received-blueprint display name (from `Game.log`) to its catalog
/// `blueprint_record_guid`s.
///
/// The log carries the in-game UI string, which is **sc-langpatch's patched
/// name** when installed: its `weapon_enhancer` / `component_grades` modules
/// *add* tokens (a ship-weapon size `"S3 …"`, a manufacturer-grade code
/// `"IND2B …"`), while Hearth's catalog has the vanilla p4k name. Because
/// those edits only *add* whole tokens, the vanilla name is a contiguous
/// whole-word run inside the log name.
///
/// Strategy: exact normalized match first (covers vanilla sessions — the 901
/// backups span pre- and post-sc-langpatch states); then the **longest
/// contiguous whole-word run** of the log name that is itself a catalog name.
/// Whole-word alignment avoids `"Bolt"` matching inside `"Deadbolt"`; longest
/// wins so the most specific name is picked; an ambiguous tie resolves to
/// nothing rather than guess. Verified to take a real 901-backup scan to
/// 65/65 resolved.
fn resolve_blueprint_guids<'a>(
    index: &'a HashMap<String, Vec<String>>,
    name: &str,
) -> Option<&'a Vec<String>> {
    let norm = normalize_bp_name(name);
    if let Some(guids) = index.get(&norm) {
        return Some(guids);
    }
    let words: Vec<&str> = norm.split_whitespace().collect();
    let mut best: Option<(usize, &'a Vec<String>)> = None;
    let mut ambiguous = false;
    for start in 0..words.len() {
        // Longest run from this start first; the full-length run equals `norm`
        // which already missed, so sub-runs are what we test.
        for end in (start + 1..=words.len()).rev() {
            let candidate = words[start..end].join(" ");
            let Some(guids) = index.get(&candidate) else { continue };
            let len = end - start;
            match best {
                Some((best_len, _)) if len > best_len => {
                    best = Some((len, guids));
                    ambiguous = false;
                }
                Some((best_len, best_guids)) if len == best_len && !std::ptr::eq(best_guids, guids) => {
                    ambiguous = true;
                }
                None => best = Some((len, guids)),
                _ => {}
            }
        }
    }
    if ambiguous {
        return None;
    }
    best.map(|(_, guids)| guids)
}

/// Tell the frontend that owned blueprints changed behind its back — live sync
/// reconcile, or the sensor auto-marking — so it can re-pull the owned set and
/// keep the catalog count current without a restart.
fn emit_ownership_changed(app: &tauri::AppHandle) {
    if let Err(e) = app.emit("ownership-changed", ()) {
        tracing::warn!("failed to emit ownership-changed: {e}");
    }
}

/// Tell the frontend the active account/scope changed (e.g. an auto-applied
/// rename swapped which account row is active) so the sidebar re-reads it.
fn emit_scope_changed(app: &tauri::AppHandle) {
    if let Err(e) = app.emit("active-scope-changed", ()) {
        tracing::warn!("failed to emit active-scope-changed: {e}");
    }
}

/// Scan a `Game.log` for the session's logged-in handle, returning at the first
/// `User Login Success` line (it sits near the top, so this rarely reads far).
/// The fallback identity source when the launcher store has no persisted handle
/// ("Remember Me" unchecked). `None` if the file is missing or has no login line.
fn read_game_log_handle(path: &std::path::Path) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    for bytes in std::io::BufReader::new(file).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&bytes);
        if let Some(sensors::SensedEvent::SessionHandle(h)) = sensors::parse::parse_line(&line) {
            return Some(h);
        }
    }
    None
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

        // Needs the install (for the log path + the active platform to guard
        // against) and the catalog (for name → guid resolution).
        let (log_path, active_platform) = match state.discovery().await {
            Ok(d) => (sensors::game_log_path(&d.install.root), d.platform),
            Err(_) => return, // no install → nothing to sense
        };
        let name_index = match state.catalog().await {
            Ok(catalog) => build_name_index(catalog),
            Err(_) => return,
        };
        // The active handle to pollution-guard against. Uses the same fallback
        // chain as the rest of the app (launcher store → Game.log → …), so the
        // sensor works without "Remember Me" too. No resolvable handle → owned
        // writes would fail anyway; don't tail (the user can still mark manually).
        let active_handle = match state.active_handle().await {
            Ok(h) => h,
            Err(_) => {
                tracing::info!("sensor disabled: no active handle to guard against");
                return;
            }
        };

        tracing::info!(path = %log_path.display(), "Game.log sensor started");
        let mut tailer = sensors::GameLogTailer::new(log_path);
        // Session header carried across polls (the handle/platform are logged
        // once near the top; the first poll backfills the whole file).
        let mut sensed_platform: Option<Platform> = None;
        let mut sensed_handle: Option<String> = None;
        let mut ticker = tokio::time::interval(POLL);

        loop {
            ticker.tick().await;
            // Gated by the user setting (default on), checked each tick so the
            // Settings toggle takes effect within one interval without a restart.
            // While off we skip the poll entirely; re-enabling backfills whatever
            // was appended in the meantime.
            let enabled = match state.db().await {
                Ok(db) => read_bool_setting(db, SENSOR_ENABLED, true).await.unwrap_or(true),
                Err(_) => true,
            };
            if !enabled {
                continue;
            }
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
                                &sensed_handle,
                                Some(s) if s.eq_ignore_ascii_case(&active_handle)
                            );
                        if !guard_ok {
                            tracing::debug!(
                                bp = %name,
                                "sensed blueprint skipped — session doesn't match active account/platform"
                            );
                            continue;
                        }
                        match resolve_blueprint_guids(&name_index, &name) {
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
                emit_ownership_changed(&handle); // refresh the catalog's owned set
            }
            tracing::info!(
                marked = marked.len(),
                newly_owned = newly_owned.len(),
                unresolved = unresolved.len(),
                "Game.log sensing pass"
            );

            // Human-facing notification through the global funnel. Built from
            // refs before the move below; the `blueprints-sensed` event stays
            // as a (currently reserved) per-page data-refresh signal.
            if !newly_owned.is_empty() {
                let count = newly_owned.len();
                let mut body = preview_names(&marked);
                if !unresolved.is_empty() {
                    body = format!("{body} · {} not recognised", unresolved.len());
                }
                notify::notify(
                    &handle,
                    notify::Notification::success(format!(
                        "Marked {count} blueprint{} owned",
                        plural(count)
                    ))
                    .with_body(body)
                    .with_action("View catalog", "/"),
                );
            } else if !unresolved.is_empty() {
                let count = unresolved.len();
                notify::notify(
                    &handle,
                    notify::Notification::warning(format!(
                        "{count} sensed blueprint{} not recognised",
                        plural(count)
                    ))
                    .with_body(preview_names(&unresolved)),
                );
            }

            if let Err(e) = handle.emit(
                "blueprints-sensed",
                BlueprintsSensed { marked, newly_owned, unresolved },
            ) {
                tracing::warn!("failed to emit blueprints-sensed: {e}");
            }
        }
    });
}

/// Startup handle-rename detection. The RSI launcher reports the *current*
/// handle; if it differs from what we ran against last time and isn't already an
/// established account, the new handle is either a rename of an existing account
/// or a genuinely separate one. We disambiguate with the one immutable anchor —
/// the UEE citizen record, scraped from the public profile. A confirmed match
/// auto-applies the rename (old handle → former) and surfaces a notification;
/// an inconclusive case is left for manual handling in Settings → Account.
///
/// Network is touched only on the changed-handle path — steady-state startups
/// (same handle as last run) return before any scrape. Best-effort throughout:
/// any error just means no auto-detection this launch.
fn spawn_rename_check(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = rename_check(&handle).await {
            tracing::warn!("rename check skipped: {e:#}");
        }
    });
}

async fn rename_check(app: &tauri::AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let db = state.db().await?;

    // First-launch identity capture is onboarding's job; only run afterwards.
    if !read_bool_setting(db, ONBOARDING_COMPLETED, false).await? {
        return Ok(());
    }

    // The active handle — launcher store, or the Game.log / fallback chain when
    // "Remember Me" is off (so rename detection works without it too). No
    // resolvable identity → nothing to do.
    let current_handle = match state.active_handle().await {
        Ok(h) => h,
        Err(_) => return Ok(()),
    };

    // Unchanged since last run → fast path, no scrape.
    let last = hearth_storage::get_setting(db, LAST_ACTIVE_HANDLE)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if last.as_deref() == Some(current_handle.as_str()) {
        return Ok(());
    }

    // If the launcher handle is already an established (anchored) account, this
    // is a plain account switch, not a rename.
    let known = hearth_storage::get_account_by_handle(db, &current_handle)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if known.as_ref().is_some_and(|a| a.citizen_record.is_some()) {
        remember_active_handle(db, &current_handle).await;
        return Ok(());
    }

    // Candidate rename sources: other accounts carrying an anchor to match.
    let others: Vec<Account> = hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .into_iter()
        .filter(|a| !a.handle.eq_ignore_ascii_case(&current_handle))
        .collect();
    let anchored: Vec<&Account> = others.iter().filter(|a| a.citizen_record.is_some()).collect();

    if anchored.is_empty() {
        // Nothing to confirm against. If there's exactly one prior account, hint
        // a possible rename for manual resolution; otherwise stay silent.
        if others.len() == 1 {
            notify_possible_rename(app, &others[0].handle, &current_handle);
        }
        remember_active_handle(db, &current_handle).await;
        return Ok(());
    }

    // Scrape the public profile for the immutable citizen record.
    let info = match identity::fetch_profile(&current_handle).await {
        Ok(info) => info,
        Err(e) => {
            tracing::info!("rename check: profile fetch failed for {current_handle}: {e}");
            if others.len() == 1 {
                notify_possible_rename(app, &others[0].handle, &current_handle);
            }
            remember_active_handle(db, &current_handle).await;
            return Ok(());
        }
    };

    match anchored.iter().find(|a| a.citizen_record == Some(info.citizen_record)) {
        Some(src) => {
            let (src_id, old_handle) = (src.id, src.handle.clone());
            hearth_storage::apply_rename(db, src_id, &current_handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
            // Refresh anchors + last_verified from the scrape we just did.
            hearth_storage::update_account_anchors(db, src_id, info.citizen_record, &info.enlisted)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
            tracing::info!(from = %old_handle, to = %current_handle, "auto-applied handle rename");
            notify::notify(
                app,
                notify::Notification::info(format!("Renamed: @{old_handle} → @{current_handle}"))
                    .with_body(
                        "Matched by your RSI profile and merged automatically. \
                         Not you? Manage accounts in Settings.",
                    )
                    .with_action("Open settings", "/settings"),
            );
            // The active account (and its owned set) changed behind the UI.
            emit_ownership_changed(app);
            emit_scope_changed(app);
        }
        None => {
            // Scraped, but no anchor matched — a genuinely separate account.
            // Capture its anchors (we have them) so a future rename of *this*
            // one is confident, then leave it as its own account.
            if let Some(acct) = hearth_storage::get_account_by_handle(db, &current_handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?
            {
                let _ = hearth_storage::update_account_anchors(
                    db,
                    acct.id,
                    info.citizen_record,
                    &info.enlisted,
                )
                .await;
            }
        }
    }

    remember_active_handle(db, &current_handle).await;
    Ok(())
}

/// Persist the launcher handle we just ran against, so the next startup's rename
/// check short-circuits unless it actually changes. Best-effort.
async fn remember_active_handle(db: &DbPool, handle: &str) {
    if let Err(e) = hearth_storage::set_setting(db, LAST_ACTIVE_HANDLE, handle).await {
        tracing::warn!("could not record last active handle: {e:#}");
    }
}

/// Non-blocking nudge when a handle changed but we couldn't anchor-confirm a
/// rename (no stored anchor, or the profile scrape failed). Points to the
/// Accounts UI where the user can merge manually.
fn notify_possible_rename(app: &tauri::AppHandle, old: &str, new: &str) {
    notify::notify(
        app,
        notify::Notification::info(format!("New handle @{new} detected"))
            .with_body(format!(
                "If you renamed from @{old}, merge them in Settings so your blueprints \
                 follow. If this is a different account, you can ignore this."
            ))
            .with_action("Open settings", "/settings"),
    );
}

/// On startup, if live sync is enabled, fetch + reconcile once after the
/// catalog is warm. The result surfaces through the notification funnel (the
/// only feedback channel here — there's no UI caller). Disabled / unreadable
/// setting → silent no-op.
#[cfg(feature = "live-sync")]
fn spawn_live_sync(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let Ok(db) = state.db().await else { return };
        if !read_bool_setting(db, LIVE_SYNC_ENABLED, false).await.unwrap_or(false) {
            return;
        }
        // The catalog is needed to resolve server ids → owned guids.
        if state.catalog().await.is_err() {
            return;
        }
        let _ = live_sync_impl(&handle, state.inner()).await;
    });
}

/// Initialise logging: a console layer (visible in `tauri dev`) **and** a daily
/// rotating file under `<app_data_root>/logs/hearth.<date>.log` (last 14 kept) —
/// the file is what users/friends can send when something goes wrong, since a
/// release build has no console. Quiet by default (warnings everywhere + our own
/// info+); override with `RUST_LOG`. Returns the writer guard, which must be
/// held for the process lifetime or buffered file logs are lost on exit.
fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,hearth_lib=info"));
    let console = fmt::layer().with_target(false);

    let logs_dir = app_data_root().join("logs");
    let file = std::fs::create_dir_all(&logs_dir).ok().and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("hearth")
            .filename_suffix("log")
            .max_log_files(14)
            .build(&logs_dir)
            .ok()
    });

    match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file_layer = fmt::layer().with_ansi(false).with_writer(writer);
            tracing_subscriber::registry()
                .with(filter)
                .with(console)
                .with(file_layer)
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry().with(filter).with(console).init();
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Keep the guard alive for the whole process so buffered file logs flush.
    let _log_guard = init_logging();

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
            spawn_rename_check(app.handle().clone());
            #[cfg(feature = "live-sync")]
            spawn_live_sync(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_exact_and_additive_langpatch_edits() {
        let mut index: HashMap<String, Vec<String>> = HashMap::new();
        index.insert("aril arms".into(), vec!["g-aril".into()]);
        index.insert("attrition-3 repeater".into(), vec!["g-attr".into()]);
        index.insert("citadel".into(), vec!["g-cit".into()]);
        index.insert("deadbolt iii cannon".into(), vec!["g-dead".into()]);
        index.insert("bolt".into(), vec!["g-bolt".into()]);

        // Exact (FPS gear — sc-langpatch doesn't prefix these).
        assert_eq!(resolve_blueprint_guids(&index, "Aril Arms"), Some(&vec!["g-aril".to_string()]));
        // Added ship-weapon size token → longest whole-word run matches.
        assert_eq!(
            resolve_blueprint_guids(&index, "S3 Attrition-3 Repeater"),
            Some(&vec!["g-attr".to_string()])
        );
        // Added manufacturer-grade token.
        assert_eq!(resolve_blueprint_guids(&index, "IND2B Citadel"), Some(&vec!["g-cit".to_string()]));
        // Whole-word alignment: "Bolt" must NOT match inside "Deadbolt".
        assert_eq!(
            resolve_blueprint_guids(&index, "S3 Deadbolt III Cannon"),
            Some(&vec!["g-dead".to_string()])
        );
        // Genuine miss.
        assert!(resolve_blueprint_guids(&index, "Totally Unknown").is_none());
    }

    #[test]
    fn reads_handle_from_game_log() {
        // The "Remember Me" off fallback: pull the session handle straight from
        // the live Game.log's login line.
        let path = std::env::temp_dir().join("hearth_resolve_handle_test.log");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            concat!(
                "<x>    [Cmdline* ] --envtag='PUB'\n",
                "<x> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[FallbackUser] - Time[1] [Login]\n",
                "<x> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo: \" [1] to queue. [Team]\n",
            ),
        )
        .unwrap();
        assert_eq!(read_game_log_handle(&path).as_deref(), Some("FallbackUser"));
        let _ = std::fs::remove_file(&path);

        // Missing file → None (not an error).
        let missing = std::env::temp_dir().join("hearth_nonexistent_handle_xyz.log");
        assert!(read_game_log_handle(&missing).is_none());
    }
}
