//! App-global key/value preferences and their Tauri command surface. The
//! values live in the `settings` table (one row per key); [`AppSettings`] is the
//! snapshot the Settings page reads, and every setter returns a fresh snapshot.

use hearth_storage::DbPool;

use crate::AppState;
use crate::error::AppError;

pub(crate) const LIVE_SYNC_ENABLED: &str = "live_sync_enabled";
pub(crate) const LIVE_SYNC_CONSENTED: &str = "live_sync_consented";
/// Live resource-inventory sync toggle (off by default). Independent of the
/// blueprint live-sync toggle, but shares the one-time [`LIVE_SYNC_CONSENTED`]
/// acknowledgement and the [`ONLINE_ENABLED`] master switch (both are the same
/// sc-dossier / CIG-backend connection).
pub(crate) const LIVE_INVENTORY_ENABLED: &str = "live_inventory_enabled";
pub(crate) const SENSOR_ENABLED: &str = "sensor_enabled";
pub(crate) const ONBOARDING_COMPLETED: &str = "onboarding_completed";
/// Last launcher handle we ran against — the steady-state guard for the startup
/// rename check (a network scrape only happens when this changes).
pub(crate) const LAST_ACTIVE_HANDLE: &str = "last_active_handle";
/// Master online switch (default on). When off, Hearth makes **no** network
/// calls: no public-RSI-profile lookups (identity / rename detection) and no
/// live game-service sync. Live sync's own enable flag is preserved and simply
/// inert while this is off.
pub(crate) const ONLINE_ENABLED: &str = "online_enabled";

/// App-global preferences surfaced to the Settings page.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct AppSettings {
    /// Whether this build was compiled with the `live-sync` feature at all.
    live_sync_available: bool,
    /// Runtime toggle — off by default; the user opts in.
    live_sync_enabled: bool,
    /// Live resource-inventory sync toggle — off by default; the user opts in.
    /// Shares the live-sync consent + online master switch.
    live_inventory_enabled: bool,
    /// Whether the one-time ToS consent has been acknowledged.
    live_sync_consented: bool,
    /// Game-log tracking (startup catch-up + auto-mark BPs received during
    /// play). Default off — the user opts in.
    sensor_enabled: bool,
    /// Whether the first-launch onboarding has been completed/skipped.
    onboarding_completed: bool,
    /// Master online switch (default on). Off → fully offline: no profile
    /// lookups (identity / rename detection) and no live game-service sync.
    online_enabled: bool,
}

/// Read a boolean setting, defaulting when the key is unset.
pub(crate) async fn read_bool_setting(
    db: &DbPool,
    key: &str,
    default: bool,
) -> Result<bool, AppError> {
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
        live_inventory_enabled: read_bool_setting(db, LIVE_INVENTORY_ENABLED, false).await?,
        live_sync_consented: read_bool_setting(db, LIVE_SYNC_CONSENTED, false).await?,
        sensor_enabled: read_bool_setting(db, SENSOR_ENABLED, false).await?,
        onboarding_completed: read_bool_setting(db, ONBOARDING_COMPLETED, false).await?,
        online_enabled: read_bool_setting(db, ONLINE_ENABLED, true).await?,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_settings(
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    read_settings(state.db().await?).await
}

/// Enable/disable live blueprint sync. Enabling records consent — the UI shows
/// the one-time consent dialog before calling this with `enabled = true`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_live_sync(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(
        db,
        LIVE_SYNC_ENABLED,
        if enabled { "true" } else { "false" },
    )
    .await
    .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if enabled {
        hearth_storage::set_setting(db, LIVE_SYNC_CONSENTED, "true")
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    }
    read_settings(db).await
}

/// Enable/disable live resource-inventory sync. Enabling records the shared
/// live-sync consent (the UI shows the one-time consent dialog before calling
/// this with `enabled = true`, same as `set_live_sync`).
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_live_inventory(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(
        db,
        LIVE_INVENTORY_ENABLED,
        if enabled { "true" } else { "false" },
    )
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
pub(crate) async fn set_sensor(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, SENSOR_ENABLED, if enabled { "true" } else { "false" })
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    read_settings(db).await
}

/// Master online switch. Off → Hearth makes no network calls at all: profile
/// lookups are blocked (re-verify errors, rename detection falls back to manual)
/// and live game-service sync stays inert even if it's enabled.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_online(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, ONLINE_ENABLED, if enabled { "true" } else { "false" })
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    read_settings(db).await
}

/// Mark the first-launch onboarding as completed (or skipped) so it doesn't
/// show again. Re-running it from Settings doesn't clear this.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_onboarding_complete(
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    let db = state.db().await?;
    hearth_storage::set_setting(db, ONBOARDING_COMPLETED, "true")
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    read_settings(db).await
}
