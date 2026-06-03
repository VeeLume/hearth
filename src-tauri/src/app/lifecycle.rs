//! Process lifecycle: logging init, the eager state warmup, and the Tauri
//! `run()` entry point that wires plugins, state, the IPC handler, and the
//! background tasks together.

use tauri::Manager;

use crate::sensors::live as sensing;
use crate::{identity, live_sync};

use super::ipc::{ipc_builder, typescript_exporter};
use super::paths::app_data_root;
use super::state::AppState;

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
            let (_, db) = tokio::join!(state.catalog(), state.db());
            if let Err(e) = db {
                tracing::warn!(error = %e, "warmup: db init failed");
            }
            // Seed the langpatch export so a current file exists before the
            // first ownership toggle (best-effort; logs on failure).
            state.refresh_owned_export().await;
        } else {
            // No install: at least try the DB so personal-state queries
            // get a clean DB error instead of a slow no-pool wait.
            if let Err(e) = state.db().await {
                tracing::warn!(error = %e, "warmup: db init failed (no install)");
            }
        }
    });
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
