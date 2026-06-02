# Releasing Hearth

Hearth ships as a signed Windows installer with auto-updates. A `v*` tag push
builds, signs, and publishes a GitHub Release; existing installs pick it up on
next launch.

## One-time setup

Two GitHub repository secrets must exist (Settings → Secrets and variables →
Actions):

- `TAURI_SIGNING_PRIVATE_KEY` — the **contents** of the updater private key.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the key's password (empty string if the
  key was generated without one).

The matching **public** key is committed in `src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`). The private key was generated with
`tauri signer generate` and is stored **outside the repo** — keep a backup
somewhere safe (a password manager). If it's lost, you can't sign updates for
existing installs; they'd need to reinstall from a fresh installer built with a
new key.

> The repo's `.gitignore` blocks `*.key` as a safety net. Never commit the
> private key.

## Release checklist

### 1. Update the changelog

Edit `CHANGELOG.md` — add entries under `## [Unreleased]` using **Added /
Changed / Fixed / Removed**. At release time the `## [Unreleased]` heading
becomes `## [x.y.z] - YYYY-MM-DD` (do this by hand in the same commit, or leave
`[Unreleased]` and let the next step's section extraction fall back to a generic
body).

### 2. Bump the version — three files, kept in lockstep

The version lives in three places; they must match the tag (without the `v`):

| File | Field |
|---|---|
| `Cargo.toml` (workspace root) | `[workspace.package]` → `version` |
| `src-tauri/tauri.conf.json` | top-level `version` |
| `package.json` | `version` |

All member crates inherit the workspace version (`version.workspace = true`), so
the root `Cargo.toml` is the single source for the Rust side.

### 3. Commit and tag

```bash
git add -A
git commit -m "release: v0.1.0"
git tag v0.1.0
git push origin main --tags
```

### 4. Wait for CI

The tag push triggers `.github/workflows/publish.yml`, which on `windows-latest`:

1. Extracts this version's section from `CHANGELOG.md` for the release body.
2. Builds the NSIS installer.
3. Signs the updater artifacts with the private key.
4. Creates the GitHub Release and uploads the installer + `latest.json`.

Monitor: <https://github.com/VeeLume/hearth/actions>

### 5. Verify

On the [Releases page](https://github.com/VeeLume/hearth/releases):

- the installer `.exe` is attached,
- `latest.json` is attached (auto-updates read it),
- the release body shows the changelog section.

For a meaningful release, install it and confirm it launches.

## How auto-updates work

On launch, Hearth fetches
`https://github.com/VeeLume/hearth/releases/latest/download/latest.json`
(see `plugins.updater.endpoints`). If a newer version is found, the user is
prompted; the download is verified against the public key before install, then
the app relaunches. **Update checks are skipped while Hearth is in offline mode**
(Settings → Account → Online features), to honour the "no network calls"
promise.

## Troubleshooting

- **No `latest.json` in the release** → the signing secrets are missing or the
  build skipped updater artifacts. Confirm both `TAURI_SIGNING_*` secrets exist
  and `createUpdaterArtifacts` is `true` in `tauri.conf.json`.
- **Clients don't see the update** → the public key in `tauri.conf.json` must
  match the private key that signed the release. A key mismatch fails
  verification silently.
- **Lost the private key** → generate a new pair, update the pubkey in
  `tauri.conf.json` and the GitHub secret. Existing installs won't auto-update
  across the key change — they'll need a manual reinstall.
