//! Crafting-planner commands — named projects + planned crafts.
//!
//! Thin CRUD wrappers over `hearth-storage`, scoped via [`AppState`]. The
//! reservation / coverage ledger (need vs have vs reserved vs free vs short) is
//! derived client-side from the plan + the live inventory (see
//! `src/lib/domain/plan.ts`), so there is no compute here — just the persisted
//! plan the UI renders and edits.

use hearth_core::{CraftPlanEntry, CraftProject, RecordId};

use crate::AppState;
use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_craft_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CraftProject>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::list_craft_projects(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_craft_project(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<CraftProject, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::create_craft_project(db, scope, name.trim())
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_craft_project(
    state: tauri::State<'_, AppState>,
    id: RecordId,
    name: String,
    notes: Option<String>,
) -> Result<Option<CraftProject>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::update_craft_project(db, scope, id, name.trim(), notes.as_deref())
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_craft_project(
    state: tauri::State<'_, AppState>,
    id: RecordId,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::delete_craft_project(db, scope, id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Toggle whether a project counts toward the materials rollup + reservation.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_craft_project_active(
    state: tauri::State<'_, AppState>,
    id: RecordId,
    active: bool,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::set_craft_project_active(db, scope, id, active)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Apply a manual project order — `ids` in the new display order.
#[tauri::command]
#[specta::specta]
pub(crate) async fn reorder_craft_projects(
    state: tauri::State<'_, AppState>,
    ids: Vec<RecordId>,
) -> Result<(), AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::reorder_craft_projects(db, scope, &ids)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_craft_plan(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CraftPlanEntry>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::list_craft_plan(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn add_craft_plan_entry(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
    project_id: Option<RecordId>,
) -> Result<CraftPlanEntry, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::add_craft_plan_entry(db, scope, &blueprint_guid, project_id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Overwrite a plan entry's editable fields. The UI sends the full current
/// state, so a `None` `target_quality` / `project_id` / `notes` means
/// "Base / Unsorted / no note", not "leave unchanged". `None` return ⇒ no such
/// entry in the active scope (e.g. deleted concurrently).
#[tauri::command]
#[specta::specta]
pub(crate) async fn update_craft_plan_entry(
    state: tauri::State<'_, AppState>,
    id: RecordId,
    project_id: Option<RecordId>,
    quantity: i32,
    target_quality: Option<i32>,
    notes: Option<String>,
) -> Result<Option<CraftPlanEntry>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::update_craft_plan_entry(
        db,
        scope,
        id,
        project_id,
        quantity,
        target_quality,
        notes.as_deref(),
    )
    .await
    .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Apply a manual entry order — `ids` in the new display order (one group's
/// worth, or any subset).
#[tauri::command]
#[specta::specta]
pub(crate) async fn reorder_craft_plan(
    state: tauri::State<'_, AppState>,
    ids: Vec<RecordId>,
) -> Result<(), AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::reorder_craft_plan(db, scope, &ids)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_craft_plan_entry(
    state: tauri::State<'_, AppState>,
    id: RecordId,
) -> Result<bool, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::remove_craft_plan_entry(db, scope, id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}
