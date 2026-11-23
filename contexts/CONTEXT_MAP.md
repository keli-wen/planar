# Context Map

> Single navigation and placement source of truth for `contexts/`. Read this file before reading from or writing to `contexts/`.

## Directory Structure

```text
contexts/
├── CONTEXT_MAP.md                           ← You are here (placement rules + full index)
│
├── project/                                 ← Product vision, design math, and system architecture
│   ├── overview.md                          ← Positioning, problem ("Attention Thrashing"), 1-core-3-surfaces, roadmap
│   ├── design.md                            ← Head-Middle-Tail slicing, Sync vs Regroup, burst math, visual/bandPath spec
│   └── architecture.md                      ← Crate/module contracts, SQLite WAL+FTS5 schema, HTTP /api/*, CLI/MCP parity
│
├── engineering/                             ← Repo-wide engineering workflows, guardrails, and subsystem contracts
│   ├── version-control-and-release.md       ← Branching, commit prefixes, SemVer lockstep, release checklist, git history hygiene
│   ├── hooks-and-gates.md                   ← Dual-layer guardrails (.githooks/ + Agent PreToolUse hooks + CI + hygiene checks)
│   └── ui-zero-fork.md                      ← Zero-dependency vanilla HTML/CSS/ESM frontend rules (and why Planar skips React)
```

## Placement Rules (Check Before Writing)

| Content Type | Destination | Naming Rule |
| :--- | :--- | :--- |
| Product positioning, roadmap, problem framing | `contexts/project/overview.md` | Update in place |
| Product rules, slicing algorithm, visual/band math | `contexts/project/design.md` | Update in place |
| Module boundaries, SQLite schema, HTTP/CLI/MCP contracts | `contexts/project/architecture.md` | Update in place |
| Git, commits, branching, SemVer, release & packaging SOPs | `contexts/engineering/version-control-and-release.md` | Update in place |
| Lints, git hooks, Agent `PreToolUse` hooks, CI gates | `contexts/engineering/hooks-and-gates.md` | Update in place |
| Subsystem guides & architectural trade-offs (e.g. UI, ingestors, MCP) | `contexts/engineering/<topic>.md` | Lowercase kebab-case |
| Reusable coding-agent SOPs & step-by-step runbooks | `.agents/skills/planar-dev/` | Keep rules in `contexts/`, workflows in skills |
| End-user runtime Agent Skill (shipped with Planar) | `skills/planar/SKILL.md` | Mirrors `planar` CLI & read-only MCP surface |
| GitHub-required root community/policy files | Repo root (`README.md`, `README.zh-CN.md`, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `REVIEW.md`, `LICENSE`) | Never duplicate `contexts/` details here — link instead |

## Durable Context Rules

1. **One fact, one home**: Never duplicate a rule across `AGENTS.md`, `CONTRIBUTING.md`, skills, and `contexts/`. Store the authoritative rule in `contexts/` and link to it.
2. **Register every new file**: Any new file added under `contexts/` MUST be registered in the directory tree above in the same change (`npm run check:hygiene` enforces this mechanically).
3. **Separate context and code commits**: Commit pure `contexts/` changes with the `ctx:` prefix separately from feature/bugfix code commits unless a single atomic change explicitly requires updating a contract alongside code.
4. **Clean-room open source**: Never put local personal paths (`/Users/<you>/...`), private workspace names, internal model codenames, real user transcripts, or secrets in `contexts/`.
5. **No ephemeral logs**: Do not file raw terminal output, single-run test logs, or temporary scratch notes in `contexts/`; store only verified product specs, architecture contracts, and engineering constraints.
