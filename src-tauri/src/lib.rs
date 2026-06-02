//! Tauri shell for Hearth.
//!
//! # Startup shape
//!
//! AppState splits the SC-load along the fast/slow seam, with each piece
//! in its own OnceCell so concurrent readers don't serialize on a mutex
//! and commands wait only on the data they actually need:
//!
//! - [`AppState::discovery`] — ~50ms. Install + handle + platform.
//!   Required by the sidebar scope chip and all DB-only commands
//!   (`list_owned`, `toggle_owned`, `active_scope`, …).
//! - [`AppState::catalog`] — 0.15s warm / ~30s cold. The cooked
//!   `Vec<BpView>`. Required only by `list_blueprints`.
//! - [`AppState::db`] — independent SQLite pool, initialized on first
//!   DB-touching command on Tauri's tokio runtime.
//!
//! `setup()` eagerly fires discovery + catalog + db on a background task
//! so the OnceCells are warm by the time the WebView mounts and starts
//! firing onMount IPC calls. Cold paths still pay their cost, but the
//! UI can show identity and accept clicks while the catalog builds in
//! the background.

use std::path::PathBuf;

use hearth_core::{Account, BpView, MissionView};
use hearth_storage::{DbPool, Scope};
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{Emitter, Manager};
use tauri_specta::{Builder, collect_commands};
use tokio::sync::OnceCell;

pub mod error;
pub mod export;
pub mod identity;
pub mod notify;
pub mod sc_loader;
pub mod sensors;

mod bp_resolve;
mod commands;
mod import;
mod live_sync;
mod sensing;
mod settings;

use error::AppError;
use sc_loader::Discovery;
use settings::LAST_ACTIVE_HANDLE;

// ── App state ───────────────────────────────────────────────────────────────

struct AppState {
    /// Fast install/handle bundle. ~50ms first call; lock-free after.
    /// Required for the sidebar scope chip and every DB-scoped command
    /// (they need the platform + active account, both derived from
    /// this).
    discovery: OnceCell<Discovery>,
    /// Cooked SC reference data (blueprint catalog + missions). Loaded
    /// lazily via the snapshot waterfall in `sc_loader::build_data`. Only
    /// `list_blueprints` / `list_missions` await this; other commands stay
    /// fast.
    data: OnceCell<sc_loader::CookedData>,
    /// SQLite pool, lazily initialized on first DB-needing command.
    db: OnceCell<DbPool>,
    /// Cached result of the last `scan_log_history` so `apply_log_import`
    /// doesn't re-read the ~900 backup logs. Cleared after a successful apply.
    pub(crate) import_scan: std::sync::Mutex<Vec<import::ScannedIdentity>>,
    /// Resolved active RSI handle, cached once. The launcher store only persists
    /// the identity when "Remember Me" is checked; this falls back to the live
    /// `Game.log`, the last-active handle, or a sole known account so the
    /// account-scoped app still works. See [`AppState::active_handle`].
    resolved_handle: OnceCell<String>,
}

impl AppState {
    fn new() -> Self {
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
    async fn discovery(&self) -> Result<&Discovery, AppError> {
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
    async fn data(&self) -> Result<&sc_loader::CookedData, AppError> {
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
    async fn catalog(&self) -> Result<&Vec<BpView>, AppError> {
        Ok(&self.data().await?.blueprints)
    }

    /// The cooked mission browser data (projection of [`Self::data`]).
    async fn missions(&self) -> Result<&Vec<MissionView>, AppError> {
        Ok(&self.data().await?.missions)
    }

    async fn db(&self) -> Result<&DbPool, AppError> {
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
    async fn active_handle(&self) -> Result<String, AppError> {
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
    async fn active_account(&self) -> Result<Account, AppError> {
        let handle = self.active_handle().await?;
        let db = self.db().await?;
        hearth_storage::upsert_account_by_handle(db, &handle)
            .await
            .map_err(|e| AppError::Storage(format!("{e:#}")))
    }

    async fn active_scope(&self) -> Result<Scope, AppError> {
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
    async fn refresh_owned_export(&self) {
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

/// Root of Hearth's on-disk data (DB, SC cache, langpatch export) under the
/// OS data dir.
///
/// **Dev / release isolation:** debug builds (`cargo tauri dev`) use a separate
/// `hearth-dev` namespace, so iterating on the dev build — deleting the DB on a
/// schema change, wiping the SC cache — never touches real release data. The
/// installed release binary uses `hearth`. `HEARTH_DATA_DIR` overrides both:
/// an escape hatch to point a dev build at release data, or to spin up a
/// throwaway profile.
pub(crate) fn app_data_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("HEARTH_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let namespace = if cfg!(debug_assertions) {
        "hearth-dev"
    } else {
        "hearth"
    };
    dirs::data_dir()
        .map(|d| d.join(namespace))
        .expect("OS data dir not resolvable")
}

/// `<app_data_root>/hearth.db`.
fn db_path() -> PathBuf {
    app_data_root().join("hearth.db")
}

// ── App setup ───────────────────────────────────────────────────────────────

/// Single source of truth for the IPC command list. Used both by
/// `run()` at app startup and by the `export-bindings` binary so the
/// TypeScript file can be regenerated without booting the full Tauri
/// app (which would require loading SC data).
pub fn ipc_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::blueprints::list_blueprints,
        commands::missions::list_missions,
        commands::missions::missions_by_blueprint,
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
        import::scan_log_history,
        import::apply_log_import,
        commands::catalog::predicted_load_tier,
        settings::get_settings,
        settings::set_live_sync,
        settings::set_sensor,
        settings::set_online,
        settings::set_onboarding_complete,
        live_sync::live_sync_now,
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
fn typescript_exporter() -> Typescript {
    Typescript::default().bigint(BigIntExportBehavior::Number)
}

/// Spawn the eager warmup: discovery, catalog, db all start filling
/// their OnceCells in parallel. By the time the WebView hydrates and
/// onMount fires (~1-4s later), the cells are likely already populated
/// and the IPC calls return instantly. Failures here are silent —
/// callers will hit the same errors on demand and report them through
/// AppError.
fn spawn_warmup(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        // Discovery first because catalog depends on it. Catalog + db
        // can then run concurrently.
        if state.discovery().await.is_ok() {
            let _ = tokio::join!(state.catalog(), state.db());
            // Seed the langpatch export so a current file exists before the
            // first ownership toggle (best-effort; logs on failure).
            state.refresh_owned_export().await;
        } else {
            // No install: at least try the DB so personal-state queries
            // get a clean DB error instead of a slow no-pool wait.
            let _ = state.db().await;
        }
    });
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

/// Tell the frontend that owned blueprints changed behind its back — live sync
/// reconcile, or the sensor auto-marking — so it can re-pull the owned set and
/// keep the catalog count current without a restart.
pub(crate) fn emit_ownership_changed(app: &tauri::AppHandle) {
    if let Err(e) = app.emit("ownership-changed", ()) {
        tracing::warn!("failed to emit ownership-changed: {e}");
    }
}

/// Tell the frontend the active account/scope changed (e.g. an auto-applied
/// rename swapped which account row is active) so the sidebar re-reads it.
pub(crate) fn emit_scope_changed(app: &tauri::AppHandle) {
    if let Err(e) = app.emit("active-scope-changed", ()) {
        tracing::warn!("failed to emit active-scope-changed: {e}");
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

/// Initialise logging: a console layer (visible in `tauri dev`) **and** a daily
/// rotating file under `<app_data_root>/logs/hearth.<date>.log` (last 14 kept) —
/// the file is what users/friends can send when something goes wrong, since a
/// release build has no console. Quiet by default (warnings everywhere + our own
/// info+); override with `RUST_LOG`. Returns the writer guard, which must be
/// held for the process lifetime or buffered file logs are lost on exit.
fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,hearth_lib=info"));
    let console = fmt::layer().with_target(false);

    let logs_dir = app_data_root().join("logs");
    let file = std::fs::create_dir_all(&logs_dir).ok().and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("hearth")
            .filename_suffix("log")
            .max_log_files(14)
            .build(&logs_dir)
            .ok()
    });

    match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file_layer = fmt::layer().with_ansi(false).with_writer(writer);
            tracing_subscriber::registry()
                .with(filter)
                .with(console)
                .with(file_layer)
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry()
                .with(filter)
                .with(console)
                .init();
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Keep the guard alive for the whole process so buffered file logs flush.
    let _log_guard = init_logging();

    let builder = ipc_builder();

    #[cfg(debug_assertions)]
    builder
        .export(typescript_exporter(), "../src/lib/bindings.ts")
        .expect("exporting TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            spawn_warmup(app.handle().clone());
            sensing::spawn_sensor(app.handle().clone());
            identity::spawn_rename_check(app.handle().clone());
            #[cfg(feature = "live-sync")]
            live_sync::spawn_live_sync(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("running tauri application");
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
