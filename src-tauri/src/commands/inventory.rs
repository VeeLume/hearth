//! Resource-inventory read command — the persisted snapshot for the active
//! scope. The *write* path (fetch + resolve + persist) lives with its heavier
//! sc-dossier logic in [`crate::inventory_sync`].

use hearth_core::InventoryStack;

use crate::AppState;
use crate::error::AppError;

/// List the active scope's stored resource inventory (resources + the
/// recipe-relevant discrete items), newest snapshot. Reads the DB only — no
/// network, so it works offline and reflects the last `inventory_sync_now`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_inventory(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<InventoryStack>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::list_inventory(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}
