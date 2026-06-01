# Hearth — agent conventions

This file is loaded by Claude Code sessions in this repository.

## What this is

Hearth is a Star Citizen blueprint / crafting / mission tracker with a community-sharing layer. Desktop (Tauri 2 + Svelte 5), eventually web and mobile, with an axum server in v2+. See `README.md` for the workspace shape.

Full design doc lives in the vault at `D:\Obsidian\Programmieren\Projects\Hearth.md` — read that before making non-trivial decisions.

## Stage of the roadmap

Currently in **v1** (personal desktop tool). v1 ships with no backend, no auth, no community features. The `hearth-server` crate exists but is empty until v2.

## Key conventions

- **Domain stays pure.** `hearth-core` contains no SQL, no Tauri types, no HTTP. Adapters live in `hearth-storage` (SQLite) and `src-tauri` (Tauri commands).
- **All sc-holotable type access goes through `hearth-core::sc_data`.** If sc-holotable churns, the blast radius is one module.
- **Sensors (log tailing, file watching) live in `src-tauri/src/sensors/`.** Empty in v1, filled in v1.5.
- **`hearth-export` is serde-only.** No logic, no I/O — just schema types the langpatch consumer can depend on without pulling in the rest of Hearth.
- **UUIDv7 / ULID for every record.** Sortable, no central authority needed, works offline. Mandatory from v1.
- **Schema reserves an `outbox` table from v1.** Unused until v2's write-queue sync arrives.
- **Pre-release migrations are edited in place.** `0001_initial.sql` is edited directly rather than adding new migration files until v1 ships. Consequence: an existing dev DB hits a sqlx migration-checksum mismatch and fails to open (which surfaces as a silent storage-init failure). **When you change the schema during dev, just delete the dev DB** — sqlx recreates it on next launch. No renaming/backup ceremony; it's throwaway dev data pre-release.
- **Dev / release data are separated by build profile** (see `app_data_root()` in `src-tauri/src/lib.rs`). Debug builds (`cargo tauri dev`) store everything (DB, SC cache, langpatch export) under `%APPDATA%/hearth-dev/`; the installed release binary uses `%APPDATA%/hearth/`. So the dev-DB delete above hits **`%APPDATA%/hearth-dev/hearth.db`** and never touches real release data. `HEARTH_DATA_DIR` overrides the root (escape hatch to point a dev build at release data, or a throwaway profile).

## Cross-repo deps

- `sc-holotable` — the umbrella crate, pinned to tag `sc-holotable/v0.9.0` in workspace Cargo.toml with `features = ["installs", "extract", "items", "missions", "crafting"]`. One pin + feature flags instead of naming the leaf crates (`sc-discovery`/`sc-extract`/`sc-items`/`sc-missions`/`sc-crafting`) and their sc-extract leaf features. Access goes through the umbrella’s feature-gated modules: `sc_holotable::install` (discovery), `::asset` (sc-extract), `::items`, `::missions`, `::crafting`, plus `::prelude`. Bump deliberately when SC data needs change. For cross-repo iteration, add a local `[patch."https://github.com/VeeLume/sc-holotable.git"]` pointing at `../sc-holotable/crates/*`, then remove it once the new tag is pushed and the pin is bumped.
- `sc-langpatch` consumes `hearth-export` (path dep when developed in parallel).

## Things not to do

- Don't write to SC's `global.ini` or `Data.p4k`. That's sc-langpatch's job. Hearth is read-only on game files.
- Don't extract SC data ad-hoc. Add types to sc-holotable instead, then consume from there. This pattern is established (the blueprint types originated in sc-langpatch and were lifted).
- Don't add features beyond the current roadmap stage without updating the vault note first.
