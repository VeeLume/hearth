//! SC-data loading commands — the loading-screen tier hint and the
//! cache-wipe debug action.

use crate::AppState;
use crate::error::AppError;
use crate::sc_loader;

/// Predict which cache tier the catalog load will use, so the loading screen
/// can name the path (and warn it may be slow). Fast — just checks snapshot
/// files on disk; needs only discovery.
#[tauri::command]
#[specta::specta]
pub(crate) async fn predicted_load_tier(
    state: tauri::State<'_, AppState>,
) -> Result<sc_loader::LoadTier, AppError> {
    let channel = state.discovery().await?.channel;
    Ok(sc_loader::predict_tier(channel))
}

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
pub(crate) async fn wipe_sc_cache(app: tauri::AppHandle) -> Result<(), AppError> {
    let cache_root = crate::app_data_root().join("cache");
    if cache_root.exists() {
        std::fs::remove_dir_all(&cache_root).map_err(|e| {
            AppError::Internal(format!("removing cache dir {}: {e}", cache_root.display()))
        })?;
        tracing::info!(path = %cache_root.display(), "wiped SC snapshot cache");
    } else {
        tracing::info!("no SC cache dir present; nothing to wipe");
    }
    // `restart()` returns `!` — the process is replaced before the
    // future ever resolves, so the Result `Ok` is never reached.
    app.restart()
}
