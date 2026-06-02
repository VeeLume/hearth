//! `AppState` — the shared, lazily-warmed application state behind every
//! Tauri command.
//!
//! The SC load is split along the fast/slow seam, each piece in its own
//! `OnceCell` so concurrent readers don't serialize on a mutex and commands
//! wait only on the data they actually need:
//!
//! - [`AppState::discovery`] — ~50ms. Install + handle + platform. Required by
//!   the sidebar scope chip and all DB-only commands (`list_owned`,
//!   `toggle_owned`, `active_scope`, …).
//! - [`AppState::catalog`] / [`AppState::missions`] — 0.15s warm / ~30s cold.
//!   The cooked reference data. Required only by `list_blueprints` /
//!   `list_missions`.
//! - [`AppState::db`] — independent SQLite pool, initialized on first
//!   DB-touching command on Tauri's tokio runtime.
//!
//! The eager warmup (see [`super::lifecycle`]) fires discovery + catalog + db
//! on a background task so the OnceCells are warm by the time the WebView
//! mounts and starts firing onMount IPC calls.

use hearth_core::{Account, BpView, MissionView};
use hearth_storage::{DbPool, Scope};
use tokio::sync::OnceCell;

use crate::error::AppError;
use crate::sc_loader::{self, CookedData, Discovery};
use crate::settings::LAST_ACTIVE_HANDLE;
use crate::{export, sensors};

use super::paths::db_path;

pub(crate) struct AppState {
    /// Fast install/handle bundle. ~50ms first call; lock-free after.
    /// Required for the sidebar scope chip and every DB-scoped command
    /// (they need the platform + active account, both derived from
    /// this).
    discovery: OnceCell<Discovery>,
    /// Cooked SC reference data (blueprint catalog + missions). Loaded
    /// lazily via the snapshot waterfall in `sc_loader::build_data`. Only
    /// `list_blueprints` / `list_missions` await this; other commands stay
    /// fast.
    data: OnceCell<CookedData>,
    /// SQLite pool, lazily initialized on first DB-needing command.
    db: OnceCell<DbPool>,
    /// Cached result of the last `scan_log_history` so `apply_log_import`
    /// doesn't re-read the ~900 backup logs. Cleared after a successful apply.
    pub(crate) import_scan: std::sync::Mutex<Vec<crate::import::ScannedIdentity>>,
    /// Resolved active RSI handle, cached once. The launcher store only persists
    /// the identity when "Remember Me" is checked; this falls back to the live
    /// `Game.log`, the last-active handle, or a sole known account so the
    /// account-scoped app still works. See [`AppState::active_handle`].
    resolved_handle: OnceCell<String>,
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            discovery: OnceCell::new(),
            data: OnceCell::new(),
            db: OnceCell::new(),
            import_scan: std::sync::Mutex::new(Vec::new()),
            resolved_handle: OnceCell::new(),
        }
    }

    /// Get the fast discovery bundle (install + handle + platform).
    /// Initialized once on first call; subsequent calls are lock-free.
    pub(crate) async fn discovery(&self) -> Result<&Discovery, AppError> {
        self.discovery
            .get_or_try_init(|| async {
                sc_loader::discover()
                    .await
                    .map_err(|e| AppError::NoInstall(format!("{e:#}")))
            })
            .await
    }

    /// Get the cooked SC reference data (catalog + missions). Awaits
    /// `discovery()` first to know which install to parse, then runs the
    /// snapshot waterfall on first call. Both products share one parse, so
    /// warming this once serves `list_blueprints` and `list_missions`.
    async fn data(&self) -> Result<&CookedData, AppError> {
        // Pull the install out before initializing so the discovery borrow
        // doesn't span the data init.
        let install = {
            let d = self.discovery().await?;
            d.install.clone()
        };
        self.data
            .get_or_try_init(|| async move {
                sc_loader::build_data(install)
                    .await
                    .map_err(|e| AppError::Internal(format!("{e:#}")))
            })
            .await
    }

    /// The cooked blueprint catalog (projection of [`Self::data`]).
    pub(crate) async fn catalog(&self) -> Result<&Vec<BpView>, AppError> {
        Ok(&self.data().await?.blueprints)
    }

    /// The cooked mission browser data (projection of [`Self::data`]).
    pub(crate) async fn missions(&self) -> Result<&Vec<MissionView>, AppError> {
        Ok(&self.data().await?.missions)
    }

    pub(crate) async fn db(&self) -> Result<&DbPool, AppError> {
        self.db
            .get_or_try_init(|| async {
                hearth_storage::open(&db_path())
                    .await
                    .map_err(|e| AppError::Storage(format!("{e:#}")))
            })
            .await
    }

    /// The active RSI handle, resolved once and cached. The launcher store only
    /// persists the identity when the user checked "Remember Me", so a raw
    /// `discovery().handle` is frequently `None` — which used to break every
    /// account-scoped command. This falls back through the live `Game.log`, the
    /// last handle we ran against, and a sole known account before giving up.
    ///
    /// Cached in a `OnceCell`: the active account is fixed per session anyway
    /// (`discovery` itself is cached; switching accounts means a restart), and
    /// caching keeps the Game.log read off the hot path. A failure isn't cached,
    /// so a later call (after the game has written its log) can still succeed.
    pub(crate) async fn active_handle(&self) -> Result<String, AppError> {
        self.resolved_handle
            .get_or_try_init(|| self.resolve_handle())
            .await
            .cloned()
    }

    async fn resolve_handle(&self) -> Result<String, AppError> {
        let d = self.discovery().await?;
        // 1. Launcher store identity (present only with "Remember Me").
        if let Some(h) = d.handle.clone() {
            return Ok(h);
        }
        // 2. The live Game.log's logged-in handle — written every session
        //    regardless of "Remember Me", so it reflects who is actually
        //    playing right now (or who played last).
        let log_path = sensors::game_log_path(&d.install.root);
        if let Some(h) = tokio::task::spawn_blocking(move || read_game_log_handle(&log_path))
            .await
            .map_err(|e| AppError::Internal(format!("game-log read join: {e}")))?
        {
            tracing::info!(handle = %h, "active handle resolved from Game.log (launcher identity unavailable)");
            return Ok(h);
        }
        // 3. The last handle we ran against, if it still maps to an account.
        let db = self.db().await?;
        if let Some(last) = hearth_storage::get_setting(db, LAST_ACTIVE_HANDLE)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?
            && hearth_storage::account_id_for_handle(db, &last)
                .await
                .map_err(|e| AppError::Storage(format!("{e:#}")))?
                .is_some()
        {
            tracing::info!(handle = %last, "active handle resolved from last-active setting");
            return Ok(last);
        }
        // 4. A single known account is unambiguous.
        let accounts = hearth_storage::list_accounts(db)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?;
        if let [only] = accounts.as_slice() {
            tracing::info!(handle = %only.handle, "active handle resolved to the sole known account");
            return Ok(only.handle.clone());
        }
        Err(AppError::Internal(
            "no RSI identity available — check \"Remember Me\" in the RSI launcher, \
             or sign in and launch the game once so Hearth can read it"
                .into(),
        ))
    }

    /// Resolve the currently-active account, bootstrapping a row from the
    /// resolved handle if needed. Returns the live `Account` row. Fast — needs
    /// only discovery + db (+ a one-time Game.log read in the fallback path),
    /// not the catalog.
    pub(crate) async fn active_account(&self) -> Result<Account, AppError> {
        let handle = self.active_handle().await?;
        let db = self.db().await?;
        hearth_storage::upsert_account_by_handle(db, &handle)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))
    }

    pub(crate) async fn active_scope(&self) -> Result<Scope, AppError> {
        let platform = self.discovery().await?.platform;
        let account = self.active_account().await?;
        Ok(Scope::new(platform, account.id))
    }

    /// Rewrite the sc-langpatch owned-blueprints export (Stage 4) from the
    /// active scope's owned set. Best-effort: a failure is logged, never
    /// surfaced, so it can't break the ownership toggle that triggered it —
    /// the file is regenerated on the next change. Called after every
    /// ownership mutation and once at warmup so langpatch's startup read
    /// finds a current file.
    pub(crate) async fn refresh_owned_export(&self) {
        if let Err(e) = self.try_refresh_owned_export().await {
            tracing::warn!("owned-blueprints export skipped: {e:#}");
        }
    }

    async fn try_refresh_owned_export(&self) -> Result<(), AppError> {
        let scope = self.active_scope().await?;
        let db = self.db().await?;
        let guids: Vec<String> = hearth_storage::list_owned(db, scope)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))?
            .into_iter()
            .map(|o| o.blueprint_guid)
            .collect();
        // FS work off the async executor.
        tokio::task::spawn_blocking(move || export::write_owned(&guids))
            .await
            .map_err(|e| AppError::Internal(format!("export join: {e}")))?
            .map_err(|e| AppError::Internal(format!("{e:#}")))
    }
}

/// Scan a `Game.log` for the session's logged-in handle, returning at the first
/// `User Login Success` line (it sits near the top, so this rarely reads far).
/// The fallback identity source when the launcher store has no persisted handle
/// ("Remember Me" unchecked). `None` if the file is missing or has no login line.
fn read_game_log_handle(path: &std::path::Path) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    for bytes in std::io::BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
    {
        let line = String::from_utf8_lossy(&bytes);
        if let Some(sensors::SensedEvent::SessionHandle(h)) = sensors::parse::parse_line(&line) {
            return Some(h);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_handle_from_game_log() {
        // The "Remember Me" off fallback: pull the session handle straight from
        // the live Game.log's login line.
        let path = std::env::temp_dir().join("hearth_resolve_handle_test.log");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            concat!(
                "<x>    [Cmdline* ] --envtag='PUB'\n",
                "<x> [Notice] <Legacy login response> [CIG-net] User Login Success - Handle[FallbackUser] - Time[1] [Login]\n",
                "<x> [Notice] <SHUDEvent_OnNotification> Added notification \"Received Blueprint: Foo: \" [1] to queue. [Team]\n",
            ),
        )
        .unwrap();
        assert_eq!(read_game_log_handle(&path).as_deref(), Some("FallbackUser"));
        let _ = std::fs::remove_file(&path);

        // Missing file → None (not an error).
        let missing = std::env::temp_dir().join("hearth_nonexistent_handle_xyz.log");
        assert!(read_game_log_handle(&missing).is_none());
    }
}
