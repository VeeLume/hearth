//! Blueprint catalog, ownership, and wishlist commands.

use hearth_core::{BpView, OwnedBlueprint, WishIntent, WishlistEntry};

use crate::AppState;
use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_blueprints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BpView>, AppError> {
    Ok(state.catalog().await?.clone())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_owned(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<OwnedBlueprint>, AppError> {
    let scope = state.active_scope().await?;
    let db = state.db().await?;
    hearth_storage::list_owned(db, scope)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn add_owned(
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
pub(crate) async fn remove_owned(
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
pub(crate) async fn toggle_owned(
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
pub(crate) async fn list_wishlist(
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
pub(crate) async fn toggle_wishlist(
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
