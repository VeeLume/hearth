//! Crafting-calculator command — the rich per-slot recipe view.

use hearth_core::CraftDetail;

use crate::AppState;
use crate::error::AppError;

/// Fetch the rich crafting view for one blueprint: named material slots, each
/// with its material, min-quality, and gameplay-property modifier curves. The
/// `/crafting` page calls this lazily when a blueprint is selected (vs.
/// `list_blueprints`, which stays lean for the catalog). `None` when the
/// blueprint has no recipe / isn't in the catalog.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_craft_detail(
    state: tauri::State<'_, AppState>,
    blueprint_guid: String,
) -> Result<Option<CraftDetail>, AppError> {
    Ok(state.craft_detail(&blueprint_guid).await?.cloned())
}
