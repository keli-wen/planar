# Reference: Version Bump & Release Runbook

> Policy source of truth: `contexts/engineering/version-control-and-release.md`. Review Tier: **T2**.

## Step 1 — Pre-Flight Cleanliness Check

```bash
git status --short
npm run verify
```

Confirm:
- Working tree is clean (or contains only intended release changes).
- `npm run lint`, `npm run check:hygiene`, `cargo test --workspace`, and `npm run test:hooks` all pass with 0 warnings.
- For the **first public release**, also audit `git log -p` for any historical private identifiers, old handles/emails, or unintended binary artifacts (see `contexts/engineering/version-control-and-release.md §4`).

## Step 2 — Lockstep Version Bump

Update all three manifests to the exact target version `X.Y.Z`:
1. `Cargo.toml` → `[workspace.package].version`
2. `package.json` → `"version"`
3. `src-tauri/tauri.conf.json` → `"version"`

Refresh lockfile & verify lockstep:

```bash
cargo check --workspace
npm run check:hygiene
```

## Step 3 — Local Daily-Driver Smoke Build (macOS)

```bash
cargo install --path crates/planar-cli
planar --version
PLANAR_DB_PATH=$(mktemp -d)/planar.db planar status --no-scan
npm run app:install && npm run app:open
```

Verify:
- `planar` CLI prints the new version and runs cleanly against an isolated DB.
- `/Applications/Planar.app` launches, Menu-Bar tray shows `Sync Pending Sessions`, and `Week / Day / Month / Settings` render cleanly.

## Step 4 — Commit & Tag (Only When Explicitly Approved by Maintainer)

Never run `git push` or create public GitHub releases without explicit maintainer confirmation:

```bash
git add Cargo.toml Cargo.lock package.json src-tauri/tauri.conf.json
git commit -m "release: vX.Y.Z"
git tag -a vX.Y.Z -m "Planar vX.Y.Z"
```

Attach release binaries (`target/release/planar` and zipped `Planar.app`) plus `shasum -a 256` checksums to the GitHub Release, grouping notes under `Added / Fixed / Changed / Context`.
