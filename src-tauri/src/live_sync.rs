//! Live blueprint sync via sc-dossier — fetch the authoritative owned-blueprint
//! set from CIG's backend and reconcile the active account's prod-scope owned
//! set to it. Behind the `live-sync` feature; gated at runtime by the live-sync
//! toggle, the one-time consent, and the master online switch.

use crate::AppState;
use crate::error::AppError;

#[cfg(feature = "live-sync")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "live-sync")]
use hearth_core::Platform;
#[cfg(feature = "live-sync")]
use hearth_storage::Scope;
#[cfg(feature = "live-sync")]
use crate::settings::{LIVE_SYNC_ENABLED, ONLINE_ENABLED, read_bool_setting};
#[cfg(feature = "live-sync")]
use crate::{emit_ownership_changed, notify, plural};

/// Outcome of one live sync, for the Settings UI + the notification body.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct LiveSyncResult {
    /// Blueprints the server reported owned.
    total: u32,
    /// Newly marked owned this sync.
    added: u32,
    /// Un-owned this sync (reconcile — the server no longer lists them).
    removed: u32,
    /// Server blueprint ids with no matching catalog entry.
    unresolved: u32,
}

/// Fetch the authoritative owned-blueprint set from CIG's backend and reconcile
/// the active account's prod-scope owned set to it. Emits a success/error
/// notification regardless of caller.
#[tauri::command]
#[specta::specta]
pub(crate) async fn live_sync_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<LiveSyncResult, AppError> {
    #[cfg(feature = "live-sync")]
    {
        // Offline mode gates live sync too (the master switch) — even if live
        // sync itself is enabled. The UI greys the controls; this guards them.
        if !read_bool_setting(state.db().await?, ONLINE_ENABLED, true).await? {
            return Err(AppError::LiveSync(
                "Hearth is in offline mode (Settings → Account → Online features)".into(),
            ));
        }
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
async fn live_sync_impl(app: &tauri::AppHandle, state: &AppState) -> Result<LiveSyncResult, AppError> {
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

/// On startup, if live sync is enabled (and online), fetch + reconcile once
/// after the catalog is warm. The result surfaces through the notification
/// funnel (there's no UI caller). Disabled / offline / unreadable → silent no-op.
#[cfg(feature = "live-sync")]
pub(crate) fn spawn_live_sync(handle: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let Ok(db) = state.db().await else { return };
        // Offline mode (master switch) suppresses startup sync even when live
        // sync is enabled.
        if !read_bool_setting(db, ONLINE_ENABLED, true).await.unwrap_or(true) {
            return;
        }
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
