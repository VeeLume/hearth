//! Game.log catch-up scan: summarize the install's session logs (the live
//! `Game.log` and the rotated `logbackups/`), keep the sessions that belong to
//! the **active** account, and mark their received blueprints owned. The batch
//! counterpart to the real-time tailer in [`super::live`] — both feed the same
//! owned set.
//!
//! Runs automatically at startup ([`catch_up`], backups only — the live file is
//! the tailer's job) and on demand via [`scan_logs_now`]. The heavy multi-hundred
//! file read reuses a persistent per-file cache, so repeat scans are near-instant.
//!
//! History is attributed to the active account only (its current handle + recorded
//! aliases); a different account's history catches up the next time it's active.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use hearth_core::{Account, Platform};
use hearth_storage::{DbPool, Scope};

use crate::error::AppError;
use crate::{AppState, app_data_root, emit_ownership_changed, notify, plural};

use super::resolve;

/// Outcome of one log scan — the Settings echo + notification body.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ScanResult {
    /// Blueprint records newly flipped to owned this scan.
    newly_owned: u32,
    /// Received-blueprint names that matched no catalog entry (name drift / locale).
    unresolved: Vec<String>,
}

/// All session-log files for a channel: the live `Game.log` + every
/// `logbackups/*.log`.
fn session_log_files(channel_dir: &Path) -> Vec<PathBuf> {
    let mut files = backup_log_files(channel_dir);
    let live = super::game_log_path(channel_dir);
    if live.exists() {
        files.push(live);
    }
    files
}

/// Just the rotated `logbackups/*.log` files (immutable once written, so fully
/// cacheable). The live `Game.log` is excluded — the tailer owns it.
fn backup_log_files(channel_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(super::log_backups_dir(channel_dir)) {
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
    summary: super::SessionSummary,
}

/// Per-file scan cache (`<file path> → cached summary`). Lets a re-scan reuse
/// every unchanged backup and only parse new ones. Regenerable; a decode failure
/// just falls back to a full scan.
fn scan_cache_path() -> PathBuf {
    app_data_root().join("import-scan-cache.json")
}

fn load_scan_cache() -> HashMap<String, CachedScan> {
    std::fs::read(scan_cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_scan_cache(cache: &HashMap<String, CachedScan>) {
    let path = scan_cache_path();
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

/// Read + summarise the given session logs (blocking — potentially hundreds of
/// files). Parses in parallel across cores, reusing cached summaries for unchanged
/// backups; the live `Game.log` (if present in `files`) is always re-parsed and
/// never cached. Rebuilds + persists the cache from this scan, pruning deleted
/// backups.
fn summarize_files(files: &[PathBuf], live: &Path) -> Vec<super::SessionSummary> {
    use rayon::prelude::*;

    let cache = load_scan_cache();

    // Per file: (summary, optional (key, entry) to persist). `None` entry = the
    // live Game.log (don't cache). A cache hit re-emits the existing entry.
    let scanned: Vec<(super::SessionSummary, Option<(String, CachedScan)>)> = files
        .par_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            let len = meta.len();
            let mtime = file_mtime_secs(&meta);
            let key = path.to_string_lossy().into_owned();
            let is_live = path.as_path() == live;

            if !is_live
                && let Some(c) = cache.get(&key)
                && c.mtime == mtime
                && c.len == len
            {
                let summary = c.summary.clone();
                return Some((
                    summary.clone(),
                    Some((key, CachedScan { mtime, len, summary })),
                ));
            }

            let file = std::fs::File::open(path).ok()?;
            let summary = super::summarize_session(std::io::BufReader::new(file));
            let entry = (!is_live).then(|| {
                (
                    key,
                    CachedScan {
                        mtime,
                        len,
                        summary: summary.clone(),
                    },
                )
            });
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

/// Handles that belong to the active account — its current handle plus recorded
/// aliases (former handles), lowercased. Used to attribute backup sessions to the
/// active account across renames.
async fn active_account_handles(
    db: &DbPool,
    account: &Account,
) -> Result<HashSet<String>, AppError> {
    let mut set = HashSet::new();
    set.insert(account.handle.to_lowercase());
    for handle in hearth_storage::list_account_aliases(db, account.id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
    {
        set.insert(handle.to_lowercase());
    }
    Ok(set)
}

/// Summarise `files`, keep prod sessions belonging to the active account, and
/// mark their received blueprints owned (active prod scope, skipping already-owned).
/// Catalog must be warm (name resolution). Refreshes the langpatch export when it
/// marked anything; the caller emits `ownership-changed` + notifies.
async fn scan_and_mark(
    state: &AppState,
    files: Vec<PathBuf>,
    live: PathBuf,
) -> Result<ScanResult, AppError> {
    let name_index = resolve::build_name_index(state.catalog().await?);
    let account = state.active_account().await?;
    let db = state.db().await?;
    let handles = active_account_handles(db, &account).await?;

    // Heavy multi-file parse off the async executor.
    let summaries = tokio::task::spawn_blocking(move || summarize_files(&files, &live))
        .await
        .map_err(|e| AppError::Internal(format!("log scan join: {e}")))?;

    // Distinct received-BP names across the active account's prod sessions.
    let mut names: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for s in summaries {
        if s.platform != Some(Platform::Prod) {
            continue;
        }
        let Some(handle) = &s.handle else { continue };
        if !handles.contains(&handle.to_lowercase()) {
            continue;
        }
        for name in s.blueprint_names {
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }

    // History is prod-only (test shards wipe), same stance as the live tail's guard.
    let scope = Scope::new(Platform::Prod, account.id);
    let mut newly_owned = 0u32;
    let mut unresolved: Vec<String> = Vec::new();
    for name in &names {
        match resolve::resolve_blueprint_guids(&name_index, name) {
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

    if newly_owned > 0 {
        state.refresh_owned_export().await; // keep the langpatch export in sync
    }
    tracing::info!(
        newly_owned,
        unresolved = unresolved.len(),
        "Game.log scan"
    );
    Ok(ScanResult {
        newly_owned,
        unresolved,
    })
}

/// Startup catch-up: scan `logbackups/` for the active account and mark owned.
/// Gated by the sensor toggle; quiet — only notifies when something was newly
/// marked. Best-effort; any failure is logged, never surfaced.
pub(crate) async fn catch_up(app: &tauri::AppHandle, state: &AppState) {
    use crate::settings::{SENSOR_ENABLED, read_bool_setting};

    let Ok(db) = state.db().await else { return };
    if !read_bool_setting(db, SENSOR_ENABLED, false)
        .await
        .unwrap_or(false)
    {
        return;
    }
    let dir = match state.discovery().await {
        Ok(d) => d.install.root.clone(),
        Err(_) => return,
    };
    let files = backup_log_files(&dir);
    if files.is_empty() {
        return;
    }
    // `live` is intentionally not in `files`, so every backup is cacheable.
    let live = super::game_log_path(&dir);
    match scan_and_mark(state, files, live).await {
        Ok(r) if r.newly_owned > 0 => {
            emit_ownership_changed(app);
            let mut n = notify::Notification::success(format!(
                "Caught up {} blueprint{} from your logs",
                r.newly_owned,
                plural(r.newly_owned as usize)
            ))
            .with_action("View catalog", "/");
            if !r.unresolved.is_empty() {
                n = n.with_body(format!("{} not recognised", r.unresolved.len()));
            }
            notify::notify(app, n);
        }
        Ok(_) => {} // nothing new — stay quiet
        Err(e) => tracing::warn!("startup log catch-up failed: {e:#}"),
    }
}

/// Manually re-scan all session logs (live + `logbackups/`) for the active
/// account and mark owned. The "Scan now" button — same role as `live_sync_now` /
/// `inventory_sync_now`. Always notifies (success even when nothing changed, so the
/// click has visible feedback).
#[tauri::command]
#[specta::specta]
pub(crate) async fn scan_logs_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ScanResult, AppError> {
    let state = state.inner();
    let dir = state.discovery().await?.install.root.clone();
    let files = session_log_files(&dir);
    let live = super::game_log_path(&dir);
    match scan_and_mark(state, files, live).await {
        Ok(r) => {
            if r.newly_owned > 0 {
                emit_ownership_changed(&app);
            }
            let mut n = notify::Notification::success(format!(
                "Scanned logs: {} blueprint{} marked",
                r.newly_owned,
                plural(r.newly_owned as usize)
            ))
            .with_action("View catalog", "/");
            if !r.unresolved.is_empty() {
                n = n.with_body(format!("{} not recognised", r.unresolved.len()));
            }
            notify::notify(&app, n);
            Ok(r)
        }
        Err(e) => {
            notify::notify(
                &app,
                notify::Notification::error("Log scan failed").with_body(e.to_string()),
            );
            Err(e)
        }
    }
}
