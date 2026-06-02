//! Account + active-scope commands (the Accounts UI and the sidebar scope
//! chip). The scope/account resolution itself lives on [`crate::AppState`];
//! these are the thin IPC wrappers plus the two UI-facing shapes they return.

use hearth_core::{Account, Platform, RecordId};

use crate::AppState;
use crate::error::AppError;
use crate::identity::fetch_profile;
use crate::settings::{ONLINE_ENABLED, read_bool_setting};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ActiveScope {
    platform: Platform,
    channel: String,
    account: Account,
}

/// An account plus its recorded past handles — the Accounts UI shape.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct AccountWithAliases {
    account: Account,
    aliases: Vec<String>,
}

/// Surface the active platform + channel + account so the UI can show
/// "PU · LIVE · @VeeLume" or similar. Fast — needs only discovery + db,
/// not the catalog, so the sidebar renders without waiting on the DCB
/// parse.
#[tauri::command]
#[specta::specta]
pub(crate) async fn active_scope(
    state: tauri::State<'_, AppState>,
) -> Result<ActiveScope, AppError> {
    let (platform, channel) = {
        let d = state.discovery().await?;
        (d.platform, d.channel.display_name().to_string())
    };
    let account = state.active_account().await?;
    Ok(ActiveScope {
        platform,
        channel,
        account,
    })
}

/// List every RSI account this desktop has known. Stage 3 wires this
/// to a picker; Stage 2.5 just exposes the data.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_accounts(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Account>, AppError> {
    let db = state.db().await?;
    hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))
}

/// Scrape `/citizens/<handle>` for the given account and write the
/// immutable anchors (`citizen_record`, `enlisted`) back to the row.
/// Refreshes `last_verified`. Returns the up-to-date `Account`.
///
/// No-ops with an error when Hearth is in offline mode (the master online
/// switch) — the UI hides the button, but this guards the network call regardless.
#[tauri::command]
#[specta::specta]
pub(crate) async fn verify_account(
    state: tauri::State<'_, AppState>,
    account_id: RecordId,
) -> Result<Account, AppError> {
    let db = state.db().await?;
    if !read_bool_setting(db, ONLINE_ENABLED, true).await? {
        return Err(AppError::Identity(
            "Hearth is in offline mode (Settings → Account → Online features)".into(),
        ));
    }
    let account = hearth_storage::get_account(db, account_id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .ok_or_else(|| AppError::Internal(format!("account {account_id} not found")))?;
    let info = fetch_profile(&account.handle)
        .await
        .map_err(|e| AppError::Identity(format!("{e:#}")))?;
    hearth_storage::update_account_anchors(db, account.id, info.citizen_record, &info.enlisted)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    hearth_storage::get_account(db, account.id)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .ok_or_else(|| AppError::Internal("account vanished after update".into()))
}

/// Accounts with their recorded past handles, for the Accounts UI.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_accounts_detailed(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    let accounts = hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    let mut out = Vec::with_capacity(accounts.len());
    for account in accounts {
        let aliases = hearth_storage::list_account_aliases(db, account.id)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        out.push(AccountWithAliases { account, aliases });
    }
    Ok(out)
}

/// Manually record a past handle for an account (rename the model didn't catch).
#[tauri::command]
#[specta::specta]
pub(crate) async fn add_account_alias(
    state: tauri::State<'_, AppState>,
    account_id: RecordId,
    handle: String,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    hearth_storage::add_account_alias(db, account_id, &handle)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    list_accounts_detailed(state).await
}

/// Merge one account into another (same person, two rows — e.g. a rename that
/// created a duplicate). Reassigns owned + wishlist data; `from` is absorbed.
/// Manual + explicit: the tool never auto-merges two accounts.
#[tauri::command]
#[specta::specta]
pub(crate) async fn merge_accounts(
    state: tauri::State<'_, AppState>,
    from: RecordId,
    into: RecordId,
) -> Result<Vec<AccountWithAliases>, AppError> {
    let db = state.db().await?;
    hearth_storage::merge_accounts(db, from, into)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    // The active account's owned set may have changed — keep the export fresh.
    state.refresh_owned_export().await;
    list_accounts_detailed(state).await
}
