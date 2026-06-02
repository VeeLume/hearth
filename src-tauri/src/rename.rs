//! Startup handle-rename detection. The RSI launcher reports the *current*
//! handle; if it differs from what we ran against last time and isn't already an
//! established account, the new handle is either a rename of an existing account
//! or a genuinely separate one. We disambiguate with the one immutable anchor —
//! the UEE citizen record, scraped from the public profile. A confirmed match
//! auto-applies the rename (old handle → former) and surfaces a notification; an
//! inconclusive case is left for manual handling in Settings → Account.
//!
//! Network is touched only on the changed-handle path — steady-state startups
//! (same handle as last run) return before any scrape. Best-effort throughout.

use hearth_core::Account;
use hearth_storage::DbPool;
use tauri::Manager;

use crate::error::AppError;
use crate::settings::{
    LAST_ACTIVE_HANDLE, ONBOARDING_COMPLETED, ONLINE_ENABLED, read_bool_setting,
};
use crate::{AppState, emit_ownership_changed, emit_scope_changed, identity, notify};

pub(crate) fn spawn_rename_check(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = rename_check(&handle).await {
            tracing::warn!("rename check skipped: {e:#}");
        }
    });
}

async fn rename_check(app: &tauri::AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let db = state.db().await?;

    // First-launch identity capture is onboarding's job; only run afterwards.
    if !read_bool_setting(db, ONBOARDING_COMPLETED, false).await? {
        return Ok(());
    }

    // The active handle — launcher store, or the Game.log / fallback chain when
    // "Remember Me" is off (so rename detection works without it too). No
    // resolvable identity → nothing to do.
    let current_handle = match state.active_handle().await {
        Ok(h) => h,
        Err(_) => return Ok(()),
    };

    // Unchanged since last run → fast path, no scrape.
    let last = hearth_storage::get_setting(db, LAST_ACTIVE_HANDLE)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if last.as_deref() == Some(current_handle.as_str()) {
        return Ok(());
    }

    // If the launcher handle is already an established (anchored) account, this
    // is a plain account switch, not a rename.
    let known = hearth_storage::get_account_by_handle(db, &current_handle)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?;
    if known.as_ref().is_some_and(|a| a.citizen_record.is_some()) {
        remember_active_handle(db, &current_handle).await;
        return Ok(());
    }

    // Candidate rename sources: other accounts carrying an anchor to match.
    let others: Vec<Account> = hearth_storage::list_accounts(db)
        .await
        .map_err(|e| AppError::Storage(format!("{e:#}")))?
        .into_iter()
        .filter(|a| !a.handle.eq_ignore_ascii_case(&current_handle))
        .collect();
    let anchored: Vec<&Account> = others
        .iter()
        .filter(|a| a.citizen_record.is_some())
        .collect();

    if anchored.is_empty() {
        // Nothing to confirm against. If there's exactly one prior account, hint
        // a possible rename for manual resolution; otherwise stay silent.
        if others.len() == 1 {
            notify_possible_rename(app, &others[0].handle, &current_handle);
        }
        remember_active_handle(db, &current_handle).await;
        return Ok(());
    }

    // Offline mode: with online features off we can't anchor-confirm, so don't
    // touch the network — fall back to the same manual hint as a failed scrape.
    if !read_bool_setting(db, ONLINE_ENABLED, true).await? {
        if others.len() == 1 {
            notify_possible_rename(app, &others[0].handle, &current_handle);
        }
        remember_active_handle(db, &current_handle).await;
        return Ok(());
    }

    // Scrape the public profile for the immutable citizen record.
    let info = match identity::fetch_profile(&current_handle).await {
        Ok(info) => info,
        Err(e) => {
            tracing::info!("rename check: profile fetch failed for {current_handle}: {e}");
            if others.len() == 1 {
                notify_possible_rename(app, &others[0].handle, &current_handle);
            }
            remember_active_handle(db, &current_handle).await;
            return Ok(());
        }
    };

    match anchored
        .iter()
        .find(|a| a.citizen_record == Some(info.citizen_record))
    {
        Some(src) => {
            let (src_id, old_handle) = (src.id, src.handle.clone());
            hearth_storage::apply_rename(db, src_id, &current_handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
            // Refresh anchors + last_verified from the scrape we just did.
            hearth_storage::update_account_anchors(db, src_id, info.citizen_record, &info.enlisted)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?;
            tracing::info!(from = %old_handle, to = %current_handle, "auto-applied handle rename");
            notify::notify(
                app,
                notify::Notification::info(format!("Renamed: @{old_handle} → @{current_handle}"))
                    .with_body(
                        "Matched by your RSI profile and merged automatically. \
                         Not you? Manage accounts in Settings.",
                    )
                    .with_action("Open settings", "/settings"),
            );
            // The active account (and its owned set) changed behind the UI.
            emit_ownership_changed(app);
            emit_scope_changed(app);
        }
        None => {
            // Scraped, but no anchor matched — a genuinely separate account.
            // Capture its anchors (we have them) so a future rename of *this* one
            // is confident, then leave it as its own account.
            if let Some(acct) = hearth_storage::get_account_by_handle(db, &current_handle)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?
            {
                let _ = hearth_storage::update_account_anchors(
                    db,
                    acct.id,
                    info.citizen_record,
                    &info.enlisted,
                )
                .await;
            }
        }
    }

    remember_active_handle(db, &current_handle).await;
    Ok(())
}

/// Persist the launcher handle we just ran against, so the next startup's rename
/// check short-circuits unless it actually changes. Best-effort.
async fn remember_active_handle(db: &DbPool, handle: &str) {
    if let Err(e) = hearth_storage::set_setting(db, LAST_ACTIVE_HANDLE, handle).await {
        tracing::warn!("could not record last active handle: {e:#}");
    }
}

/// Non-blocking nudge when a handle changed but we couldn't anchor-confirm a
/// rename (no stored anchor, or the profile scrape failed). Points to the
/// Accounts UI where the user can merge manually.
fn notify_possible_rename(app: &tauri::AppHandle, old: &str, new: &str) {
    notify::notify(
        app,
        notify::Notification::info(format!("New handle @{new} detected"))
            .with_body(format!(
                "If you renamed from @{old}, merge them in Settings so your blueprints \
                 follow. If this is a different account, you can ignore this."
            ))
            .with_action("Open settings", "/settings"),
    );
}
