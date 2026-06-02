//! Mission browser commands.

use std::collections::HashMap;

use hearth_core::{MissionRef, MissionView};

use crate::AppState;
use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_missions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MissionView>, AppError> {
    Ok(state.missions().await?.clone())
}

/// Map of `blueprint_record_guid → missions that grant it`, derived by
/// inverting the cooked mission list. Powers the wishlist's ⚐ fulfilment slot
/// ("which missions grant this blueprint?"). Awaits the same shared cooked
/// data as `list_missions` — no extra load cost when the catalog is warm.
#[tauri::command]
#[specta::specta]
pub(crate) async fn missions_by_blueprint(
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, Vec<MissionRef>>, AppError> {
    Ok(hearth_core::missions_by_blueprint(state.missions().await?))
}
