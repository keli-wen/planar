# Version Control & Release Engineering

> Single source of truth for Git conventions, branching, SemVer lockstep, pre-release gates, and local/public release packaging in `keli-wen/planar`.

---

## 1. Branching & Worktree Model

- **`master`**: Always green (`npm run verify` passes). Experimental development lands via small, review-tiered PRs (see [REVIEW.md](../../REVIEW.md)).
- **Feature / Fix branches**: `<prefix>/<short-kebab-topic>`, e.g.:
  - `feat/core-auto-link-pass`
  - `fix/ui-sync-stale-refresh`
  - `ctx/release-process`
  - `chore/ci-and-hooks`
- **Agent Worktree Isolation**: When running concurrent coding agents on Planar, isolate each implementation task in a separate git worktree (e.g. `git worktree add ../planar-<topic> -b feat/<topic>`) so parallel edits and `cargo test` runs never contend on one working directory. After PR merge, return to `master`, pull latest, and remove the finished worktree and branch.

---

## 2. Commit Message Convention

Planar adopts **Conventional Commits 1.0.0** with module-aware prefixes so commits map cleanly to architecture boundaries and changelogs:

```text
<type>[(<scope>)][!]: <concise imperative summary>

- <optional bullet explaining design rationale or trade-offs>

Refs: #<issue>
```

| Prefix (`type`) | Scope / Usage | Example |
| :--- | :--- | :--- |
| `ctx` | Durable docs under `contexts/` | `ctx: add version control and release guide` |
| `core` | `crates/planar-core/` (ingestors, SQLite, slicer, MCP, HTTP) | `core(db): lower stale digest step delta to +2` |
| `cli` | `crates/planar-cli/` subcommands and flags | `cli: add planar sync, regroup, and open commands` |
| `app` | `src-tauri/` desktop shell & tray menu | `app(tray): add one-click Sync Pending Sessions item` |
| `ui` | `ui/` zero-fork frontend and `assets/` screenshots | `ui: always stream sync to refresh grown sessions` |
| `skill` | `skills/planar/` or `.agents/skills/` | `skill(planar-dev): add release and pre-pr references` |
| `feat` / `fix` / `refactor` / `test` | Cross-cutting feature, bugfix, refactor, or test changes | `fix(core,ui): preserve clipboard resume on Linux` |
| `chore` / `ci` / `docs` / `release` | Tooling, hooks, GitHub Actions, root docs, version bumps | `release: v0.1.0` |

**Commit Splitting Discipline**:
- Keep pure `contexts/` updates in separate `ctx:` commits from code changes unless a single PR updates a tightly coupled contract (e.g., adding a new API route + updating `contexts/project/architecture.md`).
- Never mix unrelated refactors with behavioral fixes.
- Enforced locally by [`.githooks/commit-msg`](../../.githooks/commit-msg).

---

## 3. SemVer & Version Lockstep

Planar is currently in **experimental pre-1.0 (`0.x.y`)**:
- `0.1.0-alpha.N` — experimental pre-release snapshots during dogfooding.
- `0.1.0` — first public open-source release on `github.com/keli-wen/planar`.
- `0.x.y` — minor (`x`) may include schema/UI evolution (always with backward-compatible SQLite migrations); patch (`y`) is bugfix/hygiene only.

### Lockstep Files (Must Always Match)

All three version manifests MUST stay identical at every commit and release tag (`npm run check:hygiene` enforces this automatically):

1. [`Cargo.toml`](../../Cargo.toml) → `[workspace.package].version` (inherited by `planar-core`, `planar-cli`, and `src-tauri`)
2. [`package.json`](../../package.json) → `"version"`
3. [`src-tauri/tauri.conf.json`](../../src-tauri/tauri.conf.json) → `"version"`

When bumping versions, also refresh `Cargo.lock` (`cargo check --workspace`) and `ui/index.html` asset cache-bust query strings if frontend assets changed.

---

## 4. Clean-Room Pre-Release Gate

Before tagging any release (or pushing the first public commit):

1. **Full automated verification**:
   ```bash
   npm run verify
   ```
   This runs `npm run lint` (Biome + `node --check` + `cargo fmt --check` + `cargo clippy -D warnings`), `npm run check:hygiene` (privacy patterns, relative doc links, `CONTEXT_MAP.md` registration, version lockstep), `cargo test --workspace`, and `npm run test:hooks`.
2. **Git history hygiene check (especially before first public push)**:
   - Audit `git log -p` for personal paths, private project identifiers, old handles/emails, or secrets.
   - Verify `git config user.name` (`Keli Wen`) and `git config user.email` (GitHub noreply or public email) before creating public release commits.
   - If pre-release private scratch commits contain redacted identifiers, squash onto a clean-room orphan commit prior to the initial public push to `keli-wen/planar`.
3. **Visual & CLI smoke check**:
   - Inspect `npm run preview` (`?mock=1` and `?mock=1&sidebar=350`) in Light & Dark modes.
   - Verify isolated CLI smoke test against a temporary DB (`PLANAR_DB_PATH=$(mktemp -d)/planar.db cargo run -p planar-cli -- status --no-scan`).

---

## 5. Release & Packaging Channels

### 5.1 Local Daily-Driver Update (macOS)

```bash
cargo install --path crates/planar-cli     # Updates ~/.cargo/bin/planar
npm run app:install && npm run app:open    # Builds, ad-hoc signs & installs /Applications/Planar.app
```

### 5.2 GitHub Release (`vX.Y.Z`)

1. Ensure `master` is clean and `npm run verify` passes.
2. Bump lockstep versions (`Cargo.toml`, `package.json`, `src-tauri/tauri.conf.json`) in a `release: vX.Y.Z` commit.
3. Create an annotated tag: `git tag -a vX.Y.Z -m "Planar vX.Y.Z"`.
4. Build release artifacts (`cargo build --release -p planar-cli`, `npm run app:build`), verify checksums (`shasum -a 256`), and publish GitHub Release notes grouping changes by `Added / Fixed / Changed / Context`.
5. **Planned (post-v0.1)**: Automated GitHub Release workflow for signed macOS `.dmg` + universal `planar` CLI binaries and Homebrew Tap (`brew install keli-wen/tap/planar`).

For the step-by-step agent release SOP, see [`.agents/skills/planar-dev/references/release.md`](../../.agents/skills/planar-dev/references/release.md).
