# Hearth — agent conventions

This file is loaded by Claude Code sessions in this repository. It is the working
map of the codebase: what the project is, how the code is laid out, and the rules
that keep it coherent. Instructions here override default behaviour.

This file is about *the code*. The full product / design rationale and forward
roadmap live in a separate maintainer design doc — its location is recorded in the
(uncommitted) `CLAUDE.local.md`. Read that doc before making non-trivial product or
architecture decisions.

## What this is

Hearth is a personal Star Citizen blueprint / mission / wishlist tracker, with a
community-sharing layer planned for later versions. Desktop (Tauri 2 + Svelte 5),
eventually web and mobile, with an axum server from v2.

It reads the blueprint catalogue out of *your* `Data.p4k` (read-only) and exports
your owned set to a JSON file that [sc-langpatch](https://github.com/VeeLume/sc-langpatch)
consumes to grey out / hide owned blueprints in-game.

## Stage of the roadmap

**v0.1.0 shipped (2026-06-03)** — first public alpha, personal desktop tool. No
backend, no auth, no community features. The `hearth-server` crate exists but is
an empty stub until v2.

Now frozen-schema: `0001_initial.sql` is no longer edited in place (see
*Schema & migrations*). What's left of v1 is the **v1.5 remainder** — crafting-event
sensing and a manual resource inventory (both parked, not abandoned). The community
layer (friend groups → Discord communities → orgs) is v2+.

See `CHANGELOG.md` for the shipped feature set; the roadmap lives in the design doc.

## Architecture at a glance

Thin frontend, fat Rust core. Domain logic lives in `hearth-core`; transport
(Tauri command now, HTTP later) is an adapter around it. The same `hearth-core`
will link into `hearth-server` in v2, so the domain stays the single source of
truth.

```
hearth/                          Cargo workspace (Rust 2024)
├── crates/
│   ├── hearth-core/             pure domain — no SQL, no Tauri, no HTTP, no I/O
│   ├── hearth-export/           serde-only langpatch schema (no logic, no I/O)
│   ├── hearth-storage/          sqlx + SQLite repositories and migrations
│   └── hearth-server/           axum — empty stub until v2
├── src-tauri/                   Tauri desktop shell: state, commands, sensors, wiring
└── src/                         SvelteKit frontend (adapter-static)
```

### `hearth-core` (`crates/hearth-core/src/`)

Pure domain types and logic, linked by both desktop and (eventually) server.

- `types.rs` — the domain vocabulary: `Account`, `Platform { Prod, Ptu }`,
  `OwnedBlueprint`, `BpView`, `MissionView`, `WishlistEntry`/`WishIntent`,
  `Recipe`/`Ingredient`, the reward views, `RecordId`.
- `missions.rs` — mission/blueprint relations (e.g. `missions_by_blueprint`).
- `sc_data.rs` — **the single chokepoint for all sc-holotable type access.** If
  sc-holotable churns, the blast radius is this one module.
- `profile.rs` — pure parse of the RSI public citizen page into `ProfileInfo`
  (the HTTP fetch lives in `src-tauri`, not here — the domain crate stays
  network-free).

### `src-tauri/src/` — the desktop shell

`lib.rs` is deliberately thin: module wiring plus the re-exports the
`export-bindings` binary, `main.rs`, and examples depend on. The real work is in
the domain modules.

- `app/` — application core. `state.rs` (`AppState`, the shared state +
  fast/slow `OnceCell`s), `paths.rs` (`app_data_root()` and on-disk layout),
  `ipc.rs` (`ipc_builder` / `export_bindings` — the tauri-specta command wiring),
  `events.rs` (cross-cutting emit helpers, `plural`/`preview_names`), `lifecycle.rs`
  (logging, warmup, `run`).
- `commands/` — Tauri IPC handlers, the adapter boundary, grouped by domain:
  `accounts`, `blueprints`, `catalog`, `missions`. Each is a thin wrapper that
  resolves scope / db via `AppState` and calls into storage / core / `sc_loader`.
  (Command surfaces inseparable from heavier logic live with that logic instead —
  see `settings`, `sensors::import`, `live_sync`.)
- `sc_loader/` — loads SC reference data from the local install along a fast/slow
  seam with a layered snapshot cache. `discover` (fast ~50ms: launcher-store reads,
  install + `Platform` + handle), `cache` (the `catalog.cook` / `extract.snap`
  snapshot cache + `LoadTier` predicates), `cook` (parsed `Datacore` → catalog +
  missions), and the root `build_data` waterfall (processed snapshot → raw extract
  snapshot → full p4k extraction).
- `sensors/` — **Game.log tracking (format-fragile, kept local on purpose).** One
  source shaped like the other syncs (toggle + startup catch-up + manual "Scan
  now"). `parse` (pure per-line recognisers, unit-tested against real samples) and
  `tailer` (whole-file `summarize_session`/`scan_reader` + incremental
  `GameLogTailer`) are the mechanism; `live` (polling task that auto-marks
  blueprints owned during play, pollution-guarded, and kicks off the startup
  catch-up), `scan` (the cached multi-file catch-up over live + `logbackups/` for
  the **active** account, plus the `scan_logs_now` command), and `resolve`
  (received-name → catalog guid) are the app-side consumers.
- `identity/` — RSI identity. `fetch` (HTTP fetch + parse of the public citizen
  page) and `rename` (startup handle-rename detection via the immutable
  citizen-record anchor).
- `live_sync.rs` — optional authoritative owned-set sync from CIG's gRPC backend
  via the `sc-dossier` dep. **Off by default, opt-in, against SC ToS** (read-only,
  your-account-only). Gated by the `online_enabled` master switch.
- `settings.rs` — `AppSettings` + the KV `settings` table keys + setter commands.
- `export.rs` — writes the langpatch `owned-blueprints.json` (atomic tmp+rename).
- `notify.rs` — the notification funnel (Rust `notify()` → `notify` event →
  frontend toasts + center).
- `error.rs` — `AppError`, the unified command error type.

### `src/` — the frontend (SvelteKit, Svelte 5 runes)

- `lib/ipc.ts` — **the single IPC boundary.** All `invoke` calls go through here;
  `lib/bindings.ts` is the generated tauri-specta surface (do not edit by hand —
  regenerate via the `export-bindings` binary).
- `lib/state/*.svelte.ts` — runes state stores: `data` (catalog / missions /
  ownership held once per session, prefetched at startup so navigation never
  flashes), `notifications`, `onboardingStore`.
- `lib/domain/` — pure frontend helpers (`catalog.ts`, `categories.ts`).
- `lib/components/` — shared components (AccountManager, Loading,
  NotificationCenter, Onboarding, Toasts, …).
- `routes/` — `+page` (catalog), `missions/`, `wishlist/`, `resources/`,
  `settings/` (tabbed: Account · Tracking · Advanced). The root `+layout`
  prefetches data and hosts onboarding / toasts / the notification bell.

## Key conventions

- **Domain stays pure.** `hearth-core` contains no SQL, no Tauri types, no HTTP,
  no I/O. Adapters live in `hearth-storage` (SQLite), `src-tauri` (Tauri commands,
  network), and later `hearth-server` (HTTP). A profile *parse* is core; the
  profile *fetch* is `src-tauri`.
- **All sc-holotable access goes through `hearth-core::sc_data`.** One module is
  the blast radius when sc-holotable churns.
- **Don't extract SC data ad-hoc.** Add types to sc-holotable, then consume from
  there. (The blueprint types originated in sc-langpatch and were lifted upstream —
  follow that pattern.)
- **`hearth-export` is serde-only.** Just the schema types the langpatch consumer
  depends on — no logic, no I/O. The export is `{ owned: HashSet<String> }` of
  `blueprint_record_guid` hex strings (`String`, not `Guid`, so sc-langpatch needn't
  pull a second sc-holotable).
- **The IPC boundary is `src/lib/ipc.ts`.** Frontend code calls through it, not
  `invoke` directly. `bindings.ts` is generated — regenerate, don't edit.
- **UUIDv7 / ULID for every record.** Sortable, no central authority, offline-safe.
  Mandatory since v1.
- **Ownership is entity-level.** Interchangeable duplicate blueprints (same
  `crafted_entity_guid`) are marked owned/cleared together — the user can't
  distinguish them in-game. Identity/collapse logic keys on the same CIG fields a
  data bug would live in, so a CIG fix self-heals the grouping.
- **`outbox` table is reserved from v1**, unused until v2's write-queue sync.
- **Sensors are format-fragile by design** — keep Game.log logic inside
  `sensors/` so a log-format break after an SC patch is a local fix.

## Schema & migrations

- **`0001_initial.sql` is now frozen** (v0.1.0 shipped). Every later schema change
  is a **new additive migration** (`0002_…`, `0003_…`) — never an edit to `0001`.
  Editing it changes its sqlx checksum and breaks every already-installed DB.
- The *pre-release* habit of editing `0001` in place is over. (Historical note: a
  dev DB that predates a schema change hits a sqlx checksum mismatch → `open`
  returns `AppError::Storage`, surfaced on the first DB-touching command.)

## Dev vs release data

`app_data_root()` (in `src-tauri/src/app/paths.rs`) namespaces all on-disk data by
build profile:

- Debug (`pnpm tauri dev`) → `%APPDATA%\hearth-dev\`
- Installed release → `%APPDATA%\hearth\`
- `HEARTH_DATA_DIR` overrides the root (point a dev build at release data, or use a
  throwaway profile).

So the dev DB, SC cache, and langpatch export under `hearth-dev\` never touch real
release data. **If you ever need a clean dev DB after a local schema experiment,
delete `%APPDATA%\hearth-dev\hearth.db`** — sqlx recreates it on next launch. (This
applies to *throwaway dev iteration*; do not edit `0001` to force it — add a
migration.)

## Cross-repo deps

- **`sc-holotable`** — the umbrella crate, pinned to tag `sc-holotable/v0.10.0` in
  the workspace `Cargo.toml` with `features = ["installs", "extract", "items",
  "missions", "crafting", "resources", "tags"]`. One pin + feature flags instead of
  naming leaf crates. Access goes through the feature-gated modules
  (`sc_holotable::{install, asset, items, missions, crafting, resources}` + `prelude`),
  all funnelled through `hearth-core::sc_data`. For cross-repo iteration, add a local
  `[patch."https://github.com/VeeLume/sc-holotable.git"]` → `../sc-holotable/crates/*`,
  then remove it once the new tag is pushed and bump the pin.
- **`sc-dossier`** — the optional live-sync dependency, pinned to tag `v0.1.1`. A
  standalone, severable crate that talks to CIG's gRPC backend with the launcher
  session (ToS-grey, read-only, your-account-only). Kept separate so a takedown
  isolates to that repo; Hearth falls back to Game.log sensing. Consumed only by
  `src-tauri/src/live_sync.rs`.
- **`sc-langpatch`** — the export *consumer*. It takes a path dep on `hearth-export`
  when the two are developed in parallel. Hearth writes; langpatch reads.

## Things not to do

- **Don't write to SC's game files.** Hearth is read-only on `Data.p4k` and
  `global.ini`. Writing the patched language file is sc-langpatch's job.
- **Don't extract SC data ad-hoc** — add types to sc-holotable (see conventions).
- **Don't edit `0001_initial.sql`** — add a migration (see *Schema & migrations*).
- **Don't add features beyond the current roadmap stage** without updating the
  design doc first.
- **Don't hand-edit `src/lib/bindings.ts`** — it's generated by `export-bindings`.
