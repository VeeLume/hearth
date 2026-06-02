//! Cross-cutting helpers shared by the background tasks and commands: the
//! frontend-event emitters and the notification-copy formatters.

use tauri::{AppHandle, Emitter};

/// Tell the frontend that owned blueprints changed behind its back — live sync
/// reconcile, or the sensor auto-marking — so it can re-pull the owned set and
/// keep the catalog count current without a restart.
pub(crate) fn emit_ownership_changed(app: &AppHandle) {
    if let Err(e) = app.emit("ownership-changed", ()) {
        tracing::warn!("failed to emit ownership-changed: {e}");
    }
}

/// Tell the frontend the active account/scope changed (e.g. an auto-applied
/// rename swapped which account row is active) so the sidebar re-reads it.
pub(crate) fn emit_scope_changed(app: &AppHandle) {
    if let Err(e) = app.emit("active-scope-changed", ()) {
        tracing::warn!("failed to emit active-scope-changed: {e}");
    }
}

/// `""` for one, `"s"` for many — for pluralising notification copy.
pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A short, comma-joined preview of names for a notification body: the first
/// four, then `+N more`.
pub(crate) fn preview_names(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(4).map(String::as_str).collect();
    let mut s = shown.join(", ");
    if names.len() > 4 {
        s.push_str(&format!(", +{} more", names.len() - 4));
    }
    s
}
