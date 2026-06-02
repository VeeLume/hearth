//! Log-history import: scan the install's session logs (live `Game.log` +
//! `logbackups/`) for the RSI identities and blueprints they received, group them
//! by account, and — after the UI classifies them — mark those blueprints owned.
//!
//! The heavy multi-hundred-file scan runs once (cached in [`AppState`] between
//! [`scan_log_history`] and [`apply_log_import`]) and reuses a persistent
//! per-file cache so repeat imports are near-instant.

use std::collections::HashMap;
use std::path::PathBuf;

use hearth_core::{Platform, RecordId};

use crate::error::AppError;
use crate::{AppState, app_data_root, bp_resolve, sensors};

/// One RSI identity discovered across the session logs — a `(account_hint else
/// handle)` group. Cached between `scan_log_history` and `apply_log_import` so
/// the heavy log read happens once. Internal (not over the IPC boundary); the UI
/// sees the leaner [`DiscoveredIdentity`].
#[derive(Debug, Clone)]
pub(crate) struct ScannedIdentity {
    /// Stable grouping key echoed back by the UI in an [`ImportChoice`].
    key: String,
    account_hint: Option<i64>,
    /// Distinct handles seen for this identity (rename history), first-seen order.
    handles: Vec<String>,
    /// Distinct blueprint display names received across the grouped sessions.
    blueprint_names: Vec<String>,
    session_count: u32,
}

/// A discovered identity surfaced to the UI for classification.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct DiscoveredIdentity {
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
pub(crate) struct ImportChoice {
    key: String,
    /// `"existing"` (use `account_id`), `"new"` (create from the identity's
    /// primary handle), or `"ignore"`.
    action: String,
    account_id: Option<RecordId>,
}

/// Summary of an applied import.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ImportResult {
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
/// every unchanged backup and only parse new ones. Regenerable; a decode failure
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

/// Scan the install's session logs (live + `logbackups/`) and surface the RSI
/// identities found, grouped by numeric `accountId` (renames fold together).
/// Caches the full scan in-state for [`apply_log_import`].
#[tauri::command]
#[specta::specta]
pub(crate) async fn scan_log_history(
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
pub(crate) async fn apply_log_import(
    state: tauri::State<'_, AppState>,
    choices: Vec<ImportChoice>,
) -> Result<ImportResult, AppError> {
    let scan = state.import_scan.lock().unwrap().clone();
    if scan.is_empty() {
        return Err(AppError::Internal(
            "no scan to apply — run scan_log_history first".into(),
        ));
    }
    let name_index = bp_resolve::build_name_index(state.catalog().await?);
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
            match bp_resolve::resolve_blueprint_guids(&name_index, name) {
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
