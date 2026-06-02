//! On-disk locations for Hearth's data, with dev/release isolation.

use std::path::PathBuf;

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
pub(crate) fn db_path() -> PathBuf {
    app_data_root().join("hearth.db")
}
