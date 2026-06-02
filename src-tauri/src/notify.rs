//! Global notification funnel (backend side).
//!
//! Backend code that wants to surface a message to the user — the Game.log
//! sensor now, the live blueprint sync and rename detection soon — builds a
//! [`Notification`] and emits it with [`notify`]. The frontend has a matching
//! single funnel (`src/lib/notifications.svelte.ts`): every `notify` event
//! lands in one store that drives both the transient toast and the persistent
//! notification center.
//!
//! Notifications are **session-memory only** — no DB, no persistence across a
//! restart (decided for the alpha). The frontend assigns the id, timestamp and
//! read-state; the backend only describes the message.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The Tauri event name the frontend store listens on.
pub const NOTIFY_EVENT: &str = "notify";

/// Severity of a notification. Drives colour and the toast auto-dismiss policy
/// on the frontend: `info` / `success` fade, `warning` / `error` persist until
/// dismissed.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NotifLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// An optional action on a notification — a labelled link the frontend turns
/// into a "View →" affordance that navigates to `href` (an in-app route).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct NotifAction {
    pub label: String,
    pub href: String,
}

/// A user-facing notification. Built with the [`Notification::success`] /
/// `warning` / … constructors and the `with_*` builders.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Notification {
    pub level: NotifLevel,
    pub title: String,
    pub body: Option<String>,
    pub action: Option<NotifAction>,
}

impl Notification {
    pub fn new(level: NotifLevel, title: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            body: None,
            action: None,
        }
    }

    pub fn info(title: impl Into<String>) -> Self {
        Self::new(NotifLevel::Info, title)
    }
    pub fn success(title: impl Into<String>) -> Self {
        Self::new(NotifLevel::Success, title)
    }
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(NotifLevel::Warning, title)
    }
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(NotifLevel::Error, title)
    }

    /// Attach a secondary detail line.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Attach a "View →"-style action that navigates to an in-app route.
    pub fn with_action(mut self, label: impl Into<String>, href: impl Into<String>) -> Self {
        self.action = Some(NotifAction {
            label: label.into(),
            href: href.into(),
        });
        self
    }
}

/// Emit a notification to the frontend. Best-effort: a failed emit is logged,
/// never propagated — a missed notification must not break the caller.
pub fn notify(app: &AppHandle, n: Notification) {
    if let Err(e) = app.emit(NOTIFY_EVENT, n) {
        tracing::warn!("failed to emit notification: {e}");
    }
}
