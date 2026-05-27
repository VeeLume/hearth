# Hearth

Star Citizen crafting / blueprint / mission / resource tracker with a community-sharing layer.

**Status:** pre-alpha. Bootstrap stage — no usable features yet.

## What this is

A desktop app (and later web + mobile) that tracks your blueprint collection, mission completions, resources, and wishlists, with optional sharing scoped to friend groups, Discord communities, or Star Citizen orgs. Integrates with [sc-langpatch](https://github.com/VeeLume/sc-langpatch) to surface ownership state inside the patched localization.

## Stack

- **Frontend:** SvelteKit (adapter-static) + Vite
- **Desktop shell:** Tauri 2
- **Backend (v2+):** axum + sqlx + SQLite + Discord OAuth
- **SC data:** [sc-holotable](https://github.com/VeeLume/sc-holotable)

## Workspace layout

```
hearth/
├── crates/
│   ├── hearth-core/        domain types and logic (pure)
│   ├── hearth-export/      JSON schema consumed by sc-langpatch
│   ├── hearth-storage/     sqlx + SQLite repos (cache on desktop, canonical on server)
│   └── hearth-server/      axum server (v2+)
├── src-tauri/              Tauri shell (desktop binary)
└── src/                    Svelte frontend
```

## Development

```bash
pnpm install
pnpm tauri dev
```

## License

MIT
