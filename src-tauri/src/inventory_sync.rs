//! Live resource-inventory sync via sc-dossier — fetch the player's stowed
//! resources (and the discrete items that crafting recipes consume) from CIG's
//! backend, resolve their names/places against the cooked catalog, and persist
//! an authoritative snapshot for the active scope. Behind the `live-sync`
//! feature; gated at runtime by the inventory toggle, the (shared) one-time
//! consent, and the master online switch — the same posture as blueprint live
//! sync, since it's the same ToS-grey backend.

use crate::AppState;
use crate::error::AppError;

#[cfg(feature = "live-sync")]
use crate::live_sync::dossier_err_message;
#[cfg(feature = "live-sync")]
use crate::settings::{LIVE_INVENTORY_ENABLED, ONLINE_ENABLED, read_bool_setting};
#[cfg(feature = "live-sync")]
use crate::{emit_inventory_changed, notify, plural};
#[cfg(feature = "live-sync")]
use chrono::Utc;
#[cfg(feature = "live-sync")]
use hearth_core::{IngredientKind, InventoryLocationKind, InventoryStack, RecordId};
#[cfg(feature = "live-sync")]
use std::collections::{HashMap, HashSet};

/// Outcome of one inventory sync, for the Settings UI + the notification body.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct InventorySyncResult {
    /// Distinct resource stacks persisted (a material at a quality in a place).
    resources: u32,
    /// Distinct item stacks persisted (recipe-relevant discrete items).
    items: u32,
    /// Total resource SCU across all resource stacks.
    total_scu: f32,
}

/// Fetch the player's live inventory from CIG's backend and replace the active
/// scope's stored snapshot with it. Emits a success/error notification
/// regardless of caller.
#[tauri::command]
#[specta::specta]
pub(crate) async fn inventory_sync_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<InventorySyncResult, AppError> {
    #[cfg(feature = "live-sync")]
    {
        // The master switch gates inventory sync too, even when the inventory
        // toggle is on. The UI greys the controls; this guards them.
        if !read_bool_setting(state.db().await?, ONLINE_ENABLED, true).await? {
            return Err(AppError::Inventory(
                "Hearth is in offline mode (Settings → Account → Online features)".into(),
            ));
        }
        inventory_sync_impl(&app, state.inner()).await
    }
    #[cfg(not(feature = "live-sync"))]
    {
        let _ = (&app, &state);
        Err(AppError::Inventory(
            "live resource sync is not available in this build".into(),
        ))
    }
}

/// The fetch + resolve + persist, without notification (the caller notifies).
#[cfg(feature = "live-sync")]
async fn inventory_sync_run(state: &AppState) -> Result<InventorySyncResult, AppError> {
    const UA: &str = concat!("Hearth/", env!("CARGO_PKG_VERSION"));

    let dossier = sc_dossier::Dossier::from_launcher(UA)
        .await
        .map_err(|e| AppError::Inventory(dossier_err_message(&e)))?;
    let items = dossier
        .items()
        .await
        .map_err(|e| AppError::Inventory(dossier_err_message(&e)))?;

    // Resolution tables from the cooked catalog (need the catalog warm — same
    // as blueprint live sync).
    let resource_names = state.resource_names().await?;
    let location_names = state.location_names().await?;

    // Which discrete items matter: only those a crafting recipe consumes (the
    // hand-mined gems, ~hundreds), keyed by their class CRC. The recipe already
    // carries the resolved name, so reuse it to name the item stacks too —
    // there's no separate item-name map in the snapshot.
    let mut item_ingredient_names: HashMap<u32, String> = HashMap::new();
    for bp in state.catalog().await? {
        if let Some(recipe) = &bp.recipe {
            for ing in &recipe.ingredients {
                if ing.kind == IngredientKind::Item
                    && let Some(crc) = ing.crc
                    && let Some(name) = &ing.name
                {
                    item_ingredient_names.entry(crc).or_insert_with(|| name.clone());
                }
            }
        }
    }
    let item_ingredient_crcs: HashSet<u32> = item_ingredient_names.keys().copied().collect();

    // Aggregate both resources and items by (crc, quality, place) — merge
    // identical stacks while preserving the distinctions the browser shows
    // (where + per-quality amount). Resources sum SCU; items (gems) sum a unit
    // count. Quality is `Option` because a gem may carry no resource overlay.
    type StackKey = (u32, Option<u16>, InventoryLocationKind, Option<String>, Option<String>);
    let mut resource_agg: HashMap<StackKey, f64> = HashMap::new();
    let mut item_agg: HashMap<StackKey, u32> = HashMap::new();

    for item in &items {
        let (location_kind, location_name, container_geid) =
            classify_location(&item.location.context, location_names);
        // Recipe item-ingredients (the hand-mined gems) are *counted*, not
        // measured in SCU — even though the backend tags them with a resource
        // overlay whose per-gem SCU rounds to ~0.00. Match them first, by the
        // same class CRC the recipe ingredient carries, so a gem isn't filed as
        // a 0.00-SCU material. `stack_size` is the gem count.
        if item_ingredient_crcs.contains(&item.class_crc) {
            let quality = item.resource.map(|r| r.quality);
            let key = (item.class_crc, quality, location_kind, location_name, container_geid);
            *item_agg.entry(key).or_default() += item.stack_size;
        } else if let Some(res) = item.resource {
            let key = (
                res.resource_id,
                Some(res.quality),
                location_kind,
                location_name,
                container_geid,
            );
            *resource_agg.entry(key).or_default() += res.scu;
        }
    }

    let now = Utc::now();
    let scope = state.active_scope().await?;
    let mut stacks: Vec<InventoryStack> = Vec::new();
    let mut total_scu = 0.0f32;

    for ((crc, quality, location_kind, location_name, container_geid), scu) in resource_agg {
        let scu = scu as f32;
        total_scu += scu;
        stacks.push(InventoryStack {
            id: RecordId::new_v7(),
            crc,
            kind: IngredientKind::Resource,
            name: resource_names.get(&crc).cloned(),
            quality,
            scu: Some(scu),
            count: None,
            location_kind,
            location_name,
            container_geid,
            platform: scope.platform,
            account_id: scope.account_id,
            synced_at: now,
        });
    }
    let resources = stacks.len() as u32;

    for ((crc, quality, location_kind, location_name, container_geid), count) in item_agg {
        stacks.push(InventoryStack {
            id: RecordId::new_v7(),
            crc,
            kind: IngredientKind::Item,
            name: item_ingredient_names.get(&crc).cloned(),
            quality,
            scu: None,
            count: Some(count as i32),
            location_kind,
            location_name,
            container_geid,
            platform: scope.platform,
            account_id: scope.account_id,
            synced_at: now,
        });
    }
    let item_count = stacks.len() as u32 - resources;

    let db = state.db().await?;
    hearth_storage::replace_inventory(db, scope, &stacks)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;

    tracing::info!(
        wire_items = items.len(),
        resources,
        items = item_count,
        total_scu,
        "inventory sync"
    );

    Ok(InventorySyncResult {
        resources,
        items: item_count,
        total_scu,
    })
}

/// Map sc-dossier's location `Context` to the stored
/// `(kind, resolved place name, container geid)`.
#[cfg(feature = "live-sync")]
fn classify_location(
    ctx: &sc_dossier::Context,
    location_names: &HashMap<u32, String>,
) -> (InventoryLocationKind, Option<String>, Option<String>) {
    use sc_dossier::Context as C;
    match ctx {
        C::Player => (InventoryLocationKind::Player, None, None),
        C::Entitlement => (InventoryLocationKind::Entitlement, None, None),
        C::Location(crc) => (
            InventoryLocationKind::Location,
            location_names.get(crc).cloned(),
            None,
        ),
        C::Hangar(crc) => (
            InventoryLocationKind::Hangar,
            location_names.get(crc).cloned(),
            None,
        ),
        C::Container(geid) => (
            InventoryLocationKind::Container,
            None,
            Some(geid.to_string()),
        ),
        C::Other(_) => (InventoryLocationKind::Other, None, None),
    }
}

/// Run an inventory sync and emit a success/error notification either way. Used
/// by both the `inventory_sync_now` command and the startup auto-sync.
#[cfg(feature = "live-sync")]
async fn inventory_sync_impl(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<InventorySyncResult, AppError> {
    match inventory_sync_run(state).await {
        Ok(r) => {
            emit_inventory_changed(app);
            notify::notify(
                app,
                notify::Notification::success(format!(
                    "Resource sync: {} stack{}",
                    r.resources + r.items,
                    plural((r.resources + r.items) as usize)
                ))
                .with_body(format!(
                    "{} resource{} · {} item{} · {:.1} SCU",
                    r.resources,
                    plural(r.resources as usize),
                    r.items,
                    plural(r.items as usize),
                    r.total_scu
                ))
                .with_action("View resources", "/resources"),
            );
            Ok(r)
        }
        Err(e) => {
            notify::notify(
                app,
                notify::Notification::error("Resource sync failed").with_body(e.to_string()),
            );
            Err(e)
        }
    }
}

/// On startup, if inventory sync is enabled (and online), fetch + persist once
/// after the catalog is warm. The result surfaces through the notification
/// funnel (there's no UI caller). Disabled / offline / unreadable → silent no-op.
#[cfg(feature = "live-sync")]
pub(crate) fn spawn_inventory_sync(handle: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let Ok(db) = state.db().await else { return };
        if !read_bool_setting(db, ONLINE_ENABLED, true)
            .await
            .unwrap_or(true)
        {
            return;
        }
        if !read_bool_setting(db, LIVE_INVENTORY_ENABLED, false)
            .await
            .unwrap_or(false)
        {
            return;
        }
        // The catalog is needed to resolve resource_ids → names.
        if state.catalog().await.is_err() {
            return;
        }
        let _ = inventory_sync_impl(&handle, state.inner()).await;
    });
}
