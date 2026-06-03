# Changelog

All notable changes to Hearth are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/VeeLume/hearth/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/VeeLume/hearth/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/VeeLume/hearth/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/VeeLume/hearth/releases/tag/v0.1.0
