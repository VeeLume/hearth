//! v1.5 live auto-sensing: tail the active install's `Game.log` and, when the
//! logged session matches the active account + platform (pollution guard),
//! auto-mark received blueprints owned. The fragile log-parsing core lives in
//! [`crate::sensors`]; this is the app-side polling task that wires it to the DB,
//! the catalog name index, and the notification funnel.

use std::time::Duration;

use hearth_core::Platform;
use tauri::{Emitter, Manager};

use crate::settings::{SENSOR_ENABLED, read_bool_setting};
use crate::{AppState, emit_ownership_changed, notify, plural, preview_names};

use super::resolve;

/// Payload of the `blueprints-sensed` event — the per-poll data-refresh signal
/// telling the UI which blueprints were auto-marked (or failed to resolve).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
struct BlueprintsSensed {
    /// Display names that resolved to ≥1 catalog blueprint this poll.
    marked: Vec<String>,
    /// `blueprint_record_guid`s newly flipped to owned (skips already-owned).
    newly_owned: Vec<String>,
    /// Sensed names that matched no catalog blueprint (name drift / locale).
    unresolved: Vec<String>,
}

/// Tail the active install's `Game.log`, pollution-guard each session against
/// the active account + platform, and auto-mark received blueprints owned.
///
/// Best-effort throughout — no install, no handle, or a poll error just means no
/// sensing; nothing here can break the rest of the app.
pub(crate) fn spawn_sensor(handle: tauri::AppHandle) {
    const POLL: Duration = Duration::from_secs(4);

    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();

        // Needs the install (for the log path + the active platform to guard
        // against) and the catalog (for name → guid resolution).
        let (log_path, active_platform) = match state.discovery().await {
            Ok(d) => (super::game_log_path(&d.install.root), d.platform),
            Err(_) => return, // no install → nothing to sense
        };
        let name_index = match state.catalog().await {
            Ok(catalog) => resolve::build_name_index(catalog),
            Err(_) => return,
        };
        // The active handle to pollution-guard against. Uses the same fallback
        // chain as the rest of the app (launcher store → Game.log → …), so the
        // sensor works without "Remember Me" too. No resolvable handle → owned
        // writes would fail anyway; don't tail (the user can still mark manually).
        let active_handle = match state.active_handle().await {
            Ok(h) => h,
            Err(_) => {
                tracing::info!("sensor disabled: no active handle to guard against");
                return;
            }
        };

        // Startup catch-up: mark owned from rotated logbackups for the active
        // account before live tailing takes over the current Game.log. Self-gated
        // on the sensor toggle; quiet unless it marked something.
        super::scan::catch_up(&handle, state.inner()).await;

        tracing::info!(path = %log_path.display(), "Game.log sensor started");
        let mut tailer = super::GameLogTailer::new(log_path);
        // Session header carried across polls (the handle/platform are logged
        // once near the top; the first poll backfills the whole file).
        let mut sensed_platform: Option<Platform> = None;
        let mut sensed_handle: Option<String> = None;
        let mut ticker = tokio::time::interval(POLL);

        loop {
            ticker.tick().await;
            // Gated by the user setting (default off — opt-in), checked each tick
            // so the Settings toggle takes effect within one interval without a
            // restart. While off we skip the poll entirely; re-enabling backfills
            // whatever was appended in the meantime.
            let enabled = match state.db().await {
                Ok(db) => read_bool_setting(db, SENSOR_ENABLED, false)
                    .await
                    .unwrap_or(false),
                Err(_) => false,
            };
            if !enabled {
                continue;
            }
            let events = match tailer.poll() {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Game.log poll failed: {e}");
                    continue;
                }
            };
            if events.is_empty() {
                continue;
            }

            // First pass: fold session state + collect guarded blueprint hits.
            let mut to_mark: Vec<(String, Vec<String>)> = Vec::new(); // (name, guids)
            let mut unresolved: Vec<String> = Vec::new();
            for ev in events {
                match ev {
                    super::SensedEvent::SessionPlatform(p) => sensed_platform = Some(p),
                    super::SensedEvent::SessionHandle(h) => sensed_handle = Some(h),
                    // accountId isn't part of the live guard (the live session
                    // is always the active account by definition); it's used by
                    // the history import to group renamed-account sessions.
                    super::SensedEvent::SessionAccountId(_) => {}
                    super::SensedEvent::BlueprintReceived { name } => {
                        // Pollution guard: same platform AND same handle as
                        // the active account, else this log isn't ours to act on.
                        let guard_ok = sensed_platform == Some(active_platform)
                            && matches!(
                                &sensed_handle,
                                Some(s) if s.eq_ignore_ascii_case(&active_handle)
                            );
                        if !guard_ok {
                            tracing::debug!(
                                bp = %name,
                                "sensed blueprint skipped — session doesn't match active account/platform"
                            );
                            continue;
                        }
                        match resolve::resolve_blueprint_guids(&name_index, &name) {
                            Some(guids) => to_mark.push((name, guids.clone())),
                            None => unresolved.push(name),
                        }
                    }
                }
            }

            if to_mark.is_empty() && unresolved.is_empty() {
                continue;
            }

            // Second pass: mark owned (resolve scope + db once for the batch).
            let mut marked = Vec::new();
            let mut newly_owned = Vec::new();
            if !to_mark.is_empty() {
                match (state.active_scope().await, state.db().await) {
                    (Ok(scope), Ok(db)) => {
                        for (name, guids) in to_mark {
                            for guid in guids {
                                match hearth_storage::get_owned(db, scope, &guid).await {
                                    Ok(Some(_)) => {} // already owned
                                    Ok(None) => {
                                        match hearth_storage::add_owned(db, scope, &guid).await {
                                            Ok(_) => newly_owned.push(guid),
                                            Err(e) => {
                                                tracing::warn!("sensor add_owned failed: {e:#}")
                                            }
                                        }
                                    }
                                    Err(e) => tracing::warn!("sensor get_owned failed: {e:#}"),
                                }
                            }
                            marked.push(name);
                        }
                    }
                    _ => {
                        tracing::warn!("sensor could not resolve scope/db; skipping this batch");
                        continue;
                    }
                }
            }

            if !newly_owned.is_empty() {
                state.refresh_owned_export().await; // keep the langpatch export in sync
                emit_ownership_changed(&handle); // refresh the catalog's owned set
            }
            tracing::info!(
                marked = marked.len(),
                newly_owned = newly_owned.len(),
                unresolved = unresolved.len(),
                "Game.log sensing pass"
            );

            // Human-facing notification through the global funnel. The
            // `blueprints-sensed` event stays as a per-page data-refresh signal.
            if !newly_owned.is_empty() {
                let count = newly_owned.len();
                let mut body = preview_names(&marked);
                if !unresolved.is_empty() {
                    body = format!("{body} · {} not recognised", unresolved.len());
                }
                notify::notify(
                    &handle,
                    notify::Notification::success(format!(
                        "Marked {count} blueprint{} owned",
                        plural(count)
                    ))
                    .with_body(body)
                    .with_action("View catalog", "/"),
                );
            } else if !unresolved.is_empty() {
                let count = unresolved.len();
                notify::notify(
                    &handle,
                    notify::Notification::warning(format!(
                        "{count} sensed blueprint{} not recognised",
                        plural(count)
                    ))
                    .with_body(preview_names(&unresolved)),
                );
            }

            if let Err(e) = handle.emit(
                "blueprints-sensed",
                BlueprintsSensed {
                    marked,
                    newly_owned,
                    unresolved,
                },
            ) {
                tracing::warn!("failed to emit blueprints-sensed: {e}");
            }
        }
    });
}
