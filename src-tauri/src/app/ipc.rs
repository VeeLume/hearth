//! IPC wiring: the single source-of-truth command list and the TypeScript
//! bindings export.

use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri_specta::{Builder, collect_commands};

use crate::sensors::scan;
use crate::{commands, inventory_sync, live_sync, settings};

/// Single source of truth for the IPC command list. Used both by
/// `run()` at app startup and by the `export-bindings` binary so the
/// TypeScript file can be regenerated without booting the full Tauri
/// app (which would require loading SC data).
pub fn ipc_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::blueprints::list_blueprints,
        commands::crafting::get_craft_detail,
        commands::missions::list_missions,
        commands::missions::missions_by_blueprint,
        commands::plan::list_craft_projects,
        commands::plan::create_craft_project,
        commands::plan::update_craft_project,
        commands::plan::delete_craft_project,
        commands::plan::set_craft_project_active,
        commands::plan::reorder_craft_projects,
        commands::plan::list_craft_plan,
        commands::plan::add_craft_plan_entry,
        commands::plan::update_craft_plan_entry,
        commands::plan::reorder_craft_plan,
        commands::plan::remove_craft_plan_entry,
        commands::blueprints::list_owned,
        commands::blueprints::add_owned,
        commands::blueprints::remove_owned,
        commands::blueprints::toggle_owned,
        commands::blueprints::list_wishlist,
        commands::blueprints::toggle_wishlist,
        commands::accounts::active_scope,
        commands::accounts::list_accounts,
        commands::accounts::verify_account,
        commands::accounts::list_accounts_detailed,
        commands::accounts::add_account_alias,
        commands::accounts::merge_accounts,
        scan::scan_logs_now,
        commands::catalog::predicted_load_tier,
        settings::get_settings,
        settings::set_live_sync,
        settings::set_live_inventory,
        settings::set_sensor,
        settings::set_online,
        settings::set_onboarding_complete,
        live_sync::live_sync_now,
        inventory_sync::inventory_sync_now,
        commands::inventory::list_inventory,
        commands::catalog::wipe_sc_cache,
    ])
}

/// Write `src/lib/bindings.ts` from the current Rust command surface.
/// Idempotent. Called from `run()` debug builds and from the
/// `export-bindings` binary.
pub fn export_bindings(out: &str) -> Result<(), specta_typescript::ExportError> {
    ipc_builder().export(typescript_exporter(), out)
}

/// Shared TS exporter config. `BigInt → Number` is safe for our small
/// i64 fields (citizen records ~7 digits, heapAccountId ~7 digits).
pub(crate) fn typescript_exporter() -> Typescript {
    Typescript::default().bigint(BigIntExportBehavior::Number)
}
