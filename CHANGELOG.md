# Changelog

All notable changes to Hearth are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-06-07

### Added

- **Crafting page — a recipe calculator.** A new **Crafting** tab. Open any
  blueprint to see its recipe as named **material slots** (Frame, Cabling, Power
  Regulator, …), each with the material, amount, minimum quality, and a **quality
  slider**. Every slot lists the **gameplay-property effects** its material's
  quality drives — Recoil Smoothness, Impact Force, and the rest — with their
  curves, recomputed live as you drag, alongside the craft time and the missions
  that grant the blueprint. Quality **presets** (Min / Base / 50% / Max) and
  **Best in stock** set every slot at once, and an **"Open at"** preference picks
  the quality recipes open at.
- **Crafting planner.** The Crafting tab leads with what you can do right now:
  **Want to make** (your ♡ wishlist items), **Ready to craft** (recipes you own
  that your materials fully cover), and **Almost** (the ones you're a material or
  two short on). The coverage sections light up once the optional live resource
  sync is on.
- **Materials coverage in the recipe.** Each slot shows **have vs need**, the
  **best quality** you hold, and **where it's stored**; the **Best in stock**
  preset snaps every slot to the best quality you own (Base for materials you
  don't have).

### Changed

- **Recipes open as a full detail view.** Clicking a blueprint in the **Catalog**
  (or Wishlist) now opens its recipe on its own page — with working browser
  back/forward — rather than expanding inline. The recipe, materials coverage, and
  quality calculator share one view across the catalog, wishlist, and crafting
  planner.
- **The Wishlist is a lean overview.** Want-item rows show their status and open
  the full recipe (with coverage) on click, instead of carrying an inline
  breakdown.
- **"Granted by" links land on the right missions.** Mission links from a recipe
  or the wishlist open the Missions tab filtered to the missions that grant it.

### Known limitations

- **Final item stats aren't computed yet.** Hearth models each material's
  per-quality modifiers, but the crafted item's absolute stats need a
  gameplay-property → base-stat link that isn't present in the static game data —
  the recipe view reserves a *Product Stats* panel for when it lands.
- The planner's **Ready to craft** / **Almost** sections need the optional live
  resource sync enabled.

## [0.3.0] - 2026-06-06

### Added

- **Missions, rebuilt.** The Missions tab is now a full mission browser rather
  than a flat list. Each mission shows its **type and faction**, an **aUEC
  payout estimate**, **required reputation** and **prerequisite chain** (with
  clickable jump links), **where it's offered** (expandable locality cards
  grouped by place kind — planets, moons, stations, lagrange points, asteroid
  clusters), and its **combat encounters** (collapsible ship pools with cargo).
  The blueprint rewards — the collection target — lead each row with an
  owned/total count.

- **Readable mission titles.** The game's `~mission(...)` placeholders are now
  filled in — a hauling contract reads "Master Rank - Direct Bulk Cargo Haul",
  a defend mission reads "…in and out of \[Stanton · Space]". Values the
  contract pins show plainly; values the game finalizes per spawn (a location
  scope, a one-of-several choice) show in `[brackets]` so it's clear they're
  best-effort, not a promise.

- **Sort, search, and page the board.** Sort by **title** or by **aUEC payout**
  (either direction), and the list now reveals rows in pages ("Show more")
  instead of stopping at a fixed cap. New **star-system** and
  **required-reputation range** filters join the existing ones in a Filters
  popover.

- **Mission families.** Same-title variants (e.g. a dozen "Master Rank" hauling
  runs across systems) collapse into one **family** row showing the combined
  blueprint-collection progress, the payout range, and the systems they cover.
  Expand it to see the variants, with the ones that still grant blueprints you
  don't own listed first. Toggle grouping off for the flat list.

### Known limitations

- Some title placeholders stay bracketed: NPC / ship **names** (the engine
  generates those at spawn) and a few recursive sub-template titles. Location
  placeholders resolve to a **system · setting** scope, not the exact spawn
  place — the concrete location isn't in the static data.
- The aUEC estimate is within ~0.04% across the validated range but drifts
  slightly on the highest-difficulty contracts; a fine-tune is planned.
- Event / seasonal missions (e.g. XenoThreat) can show stale figures and aren't
  filtered out yet.

## [0.2.0] - 2026-06-04

### Added

- **Resources page & live resource sync.** A new **Resources** tab lists your in-game resource inventory — materials grouped by name with their total amount, best quality, and an expandable per-quality / per-location breakdown (hand-mined gems like Hadanite show as a `×N` unit count, bulk materials in SCU). It's filled by an optional **live resource sync** that reads your inventory from your RSI account (opt-in, off by default; an unofficial, read-only connection, against Star Citizen's Terms of Service; shares live blueprint sync's one-time consent and the *Online features* switch). Enable it from onboarding or *Settings → Blueprint import*.
- **Wishlist resource coverage.** Want-items now show whether your synced materials cover their recipe — a have / partial badge per item, plus per-ingredient amount-vs-need, best available quality, and where each material is stored.

### Changed

- **Game-log tracking is now one source, like the syncs.** The separate "Import from game logs" flow has been folded into game-log tracking: a single toggle that catches up from your past logs (`logbackups/`) at startup, keeps marking blueprints live while you play, and offers a **Scan now** button to re-scan on demand — the same toggle / startup / manual-button model as live blueprint and resource sync. Imported history now follows your **active account** (renames included); other accounts catch up the next time you play on them. The multi-account classification screen has been removed. Tracking is now **off by default** — opt in from onboarding or *Settings → Tracking*.
- **The catalog's sync button now uses your best enabled source.** It appears whenever live blueprint sync *or* game-log tracking is on, and refreshes from live sync when available, otherwise a game-log scan.

## [0.1.2] - 2026-06-03

### Fixed

- **Recipes now list hand-mined ingredients.** Crafting recipes that need hand-mined gems (e.g. Hadanite) or other item-based ingredients previously showed only their ship-mined ore resources — the item costs were silently dropped. Recipes now show both: resources with their SCU amount and items with a unit count (`×N`).

## [0.1.1] - 2026-06-03

### Fixed

- Account setup on a cold first launch could hit a race in the local store; the upsert is now race-safe.
- The active account scope now retries on a cold start instead of failing, and scope/database errors are no longer mislabeled as something else.

### Changed

- Scope and database errors that were previously swallowed are now written to the log, making first-launch issues easier to diagnose.

## [0.1.0] - 2026-06-03

First public alpha — a personal, offline-first blueprint / mission / wishlist tracker for Star Citizen.

### Added

- **Blueprint catalogue** read from your own `Data.p4k`: the complete craftable index, grouped by category and item type, with colourway variants collapsed into expandable `owned x/y` rows and interchangeable duplicate blueprints folded into a single entry.
- **Ownership tracking** — mark blueprints owned with a `✓` toggle; stored locally, per account.
- **Mission tracking** — which missions grant which blueprints, how many rewards you still need, where each is offered, and once-only / illegal flags. A mission counts as done once you own every blueprint in its reward pool (derived from ownership, not tracked by hand).
- **Wishlist** — flag blueprints to craft (⚐) or items to own (♡); owned want-items expand to show their recipe, and rows cross-link to the missions that grant them.
- **Three ownership sources** — manual ticking, live `Game.log` sensing (local, ToS-safe), and optional **live blueprint sync** from your RSI account (opt-in, off by default; an unofficial, read-only connection, against Star Citizen's Terms of Service).
- **Accounts & rename detection** — follows the launcher's signed-in account, auto-relinks paid handle renames (confirmed against your public RSI profile), and supports manual merge of duplicates.
- **sc-langpatch integration** — writes your owned set to `%APPDATA%\hearth\exports\owned-blueprints.json` for sc-langpatch to consume.
- **Offline mode** — a single *Online features* switch disables all network use while leaving local tracking fully working.
- **First-launch onboarding** and **auto-update** (signed; checks GitHub Releases on launch, skipped in offline mode).

[Unreleased]: https://github.com/VeeLume/hearth/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/VeeLume/hearth/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/VeeLume/hearth/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/VeeLume/hearth/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/VeeLume/hearth/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/VeeLume/hearth/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/VeeLume/hearth/releases/tag/v0.1.0
