# Hearth

A personal blueprint, mission & wishlist tracker for Star Citizen.

[![Latest release](https://img.shields.io/github/v/release/VeeLume/hearth?display_name=tag)](https://github.com/VeeLume/hearth/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/VeeLume/hearth/total)](https://github.com/VeeLume/hearth/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Hearth keeps track of which Star Citizen blueprints you own, which missions still have rewards worth collecting, and what you're still hunting for — all on your own machine, no account or sign-up required. It reads the blueprint catalogue straight out of *your* `Data.p4k`, so it always matches the build you're actually on. And it feeds [sc-langpatch](https://github.com/VeeLume/sc-langpatch) so the labels you see in-game can grey out or hide the blueprints you already have.

<!-- TODO before publish: add screenshots under docs/screenshots/ and embed them here,
     e.g. the catalogue, a mission's reward pool, and the wishlist. -->

> [!NOTE]
> Hearth is **read-only on your game files** — it never writes to `Data.p4k` or `global.ini` (writing the patched language file is [sc-langpatch](https://github.com/VeeLume/sc-langpatch)'s job). Your collection lives in a local database; nothing leaves your machine unless you turn on an online feature.

## What it does

- **Blueprint catalogue** — the *complete* craftable index pulled from your install (not just mission rewards, so default-unlocked blueprints show up too), grouped by category and item type. Colourway variants of one design collapse into a single expandable row with an `owned x/y` count; interchangeable duplicate blueprints fold into one entry.
- **Ownership tracking** — tick `✓` to mark a blueprint owned. Stored locally, per account.
- **Missions** — see which missions grant which blueprints, how many of a mission's rewards you still need, where it's offered, and flags like *once-only* / *illegal*. A mission counts as "done" automatically once you own everything in its reward pool.
- **Wishlist** — flag blueprints you *want to craft* (⚐) or items you *want to own* (♡). Want-items you can already craft expand to show the recipe; wishlist rows cross-link to the missions that grant them.
- **Multiple accounts & renames** — Hearth follows whichever account your RSI launcher is signed into. It detects paid handle renames on startup and re-links them automatically (confirmed against your public RSI profile); duplicates can be merged by hand.
- **sc-langpatch integration** — Hearth writes your owned set to a small JSON file that sc-langpatch reads, so in-game blueprint labels reflect what you have.
- **Offline by default, online when you opt in** — a single *Online features* switch governs all network use.
- **Auto-update** — checks GitHub Releases on launch and installs signed updates (skipped in offline mode).

### Keeping your owned list up to date

Three complementary sources, all opt-in or local:

| Source | What it does | Risk |
|---|---|---|
| **Manual** | Tick `✓` yourself. | None. |
| **Live game-log sensing** | Watches `Game.log` while you play and marks blueprints owned the moment you receive one. | **ToS-safe** — a local read of a log file CIG writes, no network, no game-process hooking. |
| **Live blueprint sync** | Pulls your *complete*, authoritative blueprint library straight from your RSI account. | **Opt-in, off by default.** An *unofficial, read-only* connection to CIG's servers using your launcher session — **against Star Citizen's Terms of Service**. Only ever touches your own account; use at your own risk. |

## Quick start

1. **Download the installer** from the [latest release](https://github.com/VeeLume/hearth/releases/latest) — pick `Hearth_X.Y.Z_x64-setup.exe`.
2. **Run it.** Windows SmartScreen may warn you because the build isn't code-signed — click *More info* → *Run anyway*. It installs to the usual Programs folder and adds a Start-menu entry.
3. **Open Hearth.** On first launch a short setup confirms your account (read from the RSI launcher) and lets you choose how to track ownership. The blueprint catalogue is read from your install in the background — the first read can take a few seconds, or longer after a Star Citizen update.
4. **Browse and tick.** Mark what you own, build a wishlist, check which missions still owe you blueprints.

> [!TIP]
> Hearth works best with Star Citizen installed and the RSI launcher signed in at least once. Without an install you can still look around; it picks up your account and catalogue once they're there.

## Where your data lives

Everything is local: a SQLite database and the sc-langpatch export under `%APPDATA%\hearth\`. There is no server, no account, and no telemetry in this version. Turning *Online features* off makes Hearth make **no** network calls at all (profile look-ups and live sync both go inert), while still leaving local tracking fully working.

A community layer — sharing wishlists and "I can craft this for you" offers with friends, Discord communities, or Star Citizen orgs — is on the roadmap (see below) but **not** part of this release.

## FAQ

**Is this against Star Citizen's ToS?**
Hearth is read-only on your game files and never modifies them. Manual tracking and live game-log sensing are entirely local (the log is a file CIG already writes). The **only** ToS-grey feature is **live blueprint sync**, which is opt-in, off by default, clearly labelled, and only ever reads your own account. Everything else is safe.

**Why is Windows SmartScreen warning me?**
The installer isn't signed with a paid code-signing certificate. The build comes straight from the public source and CI; updates are cryptographically signed and verified against the release's `latest.json`.

**It says no account / no installation was found.**
Hearth reads your account from the RSI launcher. The launcher only persists the signed-in handle when **"Remember Me"** is checked — otherwise Hearth falls back to your current `Game.log`, so launch the game once as the intended account. Make sure Star Citizen has been installed and launched at least once.

**Can I use it fully offline?**
Yes. Turn off *Online features* (Settings → Account). Local game-log tracking and manual editing keep working; nothing touches the network.

## Roadmap

Personal-first, community-ready. Each version is useful on its own.

- **v1 — personal desktop tool (this release):** catalogue, ownership, missions, wishlist, accounts, sc-langpatch export, offline-first.
- **v1.5 — crafting & resources:** a "what can I craft right now?" view and a manual resource inventory.
- **v2+ — sharing:** an optional server so you can sync across devices and share wishlists / crafting offers with friend groups, then Discord communities and orgs.

The full design doc drives this; the workspace already carries the v2 shape (an empty `hearth-server` crate, a reserved `outbox` sync table) so later versions slot in cleanly.

## A note on AI assistance

Parts of this codebase — and this README — were written with the help of AI tools (primarily Claude Code). Every change is reviewed before it lands and the project has tests covering the storage and domain layers, but I'd rather be upfront about it than pretend otherwise. If something reads or behaves oddly, an issue or PR is very welcome.

---

## For developers

End users don't need any of this — grab the installer above.

### Stack

- **Frontend:** [Svelte 5](https://svelte.dev/) (SvelteKit, adapter-static) + [Tauri 2](https://tauri.app/)
- **Core:** Rust 2024 edition, Cargo workspace
- **Storage:** [sqlx](https://github.com/launchbadge/sqlx) + SQLite
- **SC data:** [sc-holotable](https://github.com/VeeLume/sc-holotable) (P4K + DataCore extraction)
- **Live sync (optional):** [sc-dossier](https://github.com/VeeLume/sc-dossier)
- **Backend (v2+):** axum — currently an empty stub

### Workspace layout

```
hearth/
├── crates/
│   ├── hearth-core/      pure domain types & logic (no SQL, no Tauri, no HTTP)
│   ├── hearth-export/    serde-only schema consumed by sc-langpatch (no logic, no I/O)
│   ├── hearth-storage/   sqlx + SQLite repositories and migrations
│   └── hearth-server/    axum server — empty stub until v2
├── src-tauri/            Tauri desktop shell: commands, sensors, app wiring
└── src/                  Svelte frontend
```

The domain stays pure: all SC-data access goes through `hearth-core::sc_data`, and storage / Tauri are adapters around it. Ownership is exported to sc-langpatch as a small `{ owned: [guid, …] }` JSON file written atomically to `%APPDATA%\hearth\exports\`.

### Prerequisites

- [Node.js](https://nodejs.org/) LTS and [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) stable (2024 edition)
- Windows (the desktop build targets Windows; an NSIS installer is produced)

### Build & run

```bash
pnpm install
pnpm tauri dev      # hot-reload dev build
pnpm tauri build    # release installer
```

Debug builds keep their data under `%APPDATA%\hearth-dev\` so they never touch a real install's data; `HEARTH_DATA_DIR` overrides the root.

### Test

```bash
cd src-tauri && cargo test       # workspace tests
pnpm check                       # svelte-check (types + markup)
```

### Releasing

See [RELEASING.md](RELEASING.md) for the tag-driven, signed release flow.

## License

[MIT](LICENSE)
