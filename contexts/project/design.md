# Planar — Product & Visual Design

> Authoritative specification for product mechanics, trajectory slicing, taxonomy governance, active-time math, and visual/interaction design. For high-level positioning and roadmap see [overview.md](overview.md); for crate/module contracts and schemas see [architecture.md](architecture.md).

---

## 1. Core Design Principles

1. **Lines of Work First, Sessions Second**: Group trajectories semantically into persistent **Lines of work** (`pinned` user-owned lanes first, `discovered` AI-clustered lanes second, `One-off` for standalone errands).
2. **Token-Frugal Head–Middle–Tail Slicing**: Never feed multi-megabyte raw transcripts with tool outputs to an LLM just to recap what happened:
   - **Short-session gate (`fewest_user_inputs = 3`)**: Sessions with `1–2` user messages skip LLM summarization completely and display their raw first prompt under `Short sessions` until they grow.
   - **3-Zone Window (`Opening 20` + `Middle User Prompts Only` + `Closing 40`)**: Full user + agent messages at both ends capture initial intent and final stopping state/blockers; keeping **only user prompts** in the middle preserves every mid-session topic pivot while skipping verbose agent replies; **tool outputs are always stripped**.
3. **Stable Taxonomy (`Sync` vs `Regroup`)**:
   - **`Sync` (Incremental, Safe)**: Summarizes only eligible new, grown (`+2` steps), or config-invalidated sessions and assigns them to existing or newly discovered Lines. **Never merges, splits, or renames existing Lines, and never overwrites pinned (`pinned = 1`) assignments.**
   - **`Regroup` (Explicit)**: Re-clusters and renames only unpinned *"Found by Planar"* Lines (`pinned = 0`) when explicitly requested by the user; pinned Lines remain immutable exemplars.
4. **Honest Active Agent Time**: Compute active intervals from trajectory step timestamps while clamping human idle gaps (`> IDLE_GAP_CAP_MS`, default `12 min`) and honoring explicit tool/subagent execution spans — measuring real agent work rather than how long a terminal tab sat open.
5. **Archived Retention**: Archived sessions are **never dropped** from historical metrics, 20-week contribution heatmaps, `planar status`, or already-filed Line totals/swimlanes (they collapse quietly behind `N archived` in expanded Line lists).

---

## 2. Trajectory Slicing Specification

Given a normalized step sequence $S = [s_1, s_2, \dots, s_N]$ where each step has `role ∈ {user, agent, tool}`:

1. **Strip tool payloads & internal echoes**: Discard raw `tool` payloads unconditionally and ignore any turn containing `[PLANAR_INTERNAL_DIGEST]` so local CLI summarization calls never self-ingest.
2. **Short-session gate**: Let $U = [s_i \in S \mid s_i.\text{role} = \text{user}]$. If $|U| < \text{fewest\_user\_inputs}$ (default `3`) and unassigned, mark session `short` and **stop** (0 LLM tokens).
3. **3-Zone partition**: Partition non-tool dialogue steps $M = [s_1, \dots, s_K]$ using `opening_steps` ($O = 20$) and `closing_steps` ($C = 40$):
   - **Head**: $M[1 \dots \min(O, K)]$ (both `user` and `agent`, truncated to `1200` / `1500` chars).
   - **Tail**: $M[\max(O+1, K - C + 1) \dots K]$ (both `user` and `agent`, truncated to `1200` / `1500` chars).
   - **Middle**: For $i \in (O, K - C]$, keep **only** steps where `role == user` (truncated to `1200` chars).
4. **Invalidation & pruned-log protection**:
   - Compute `slice_config_hash` over `(opening_steps, closing_steps, longest_user_chars, longest_agent_chars, fewest_user_inputs)`.
   - Re-summarize during `Sync` when unassigned, `slice_config_hash` changed, or `step_count >= last_step_index + 2`.
   - Once a source `.jsonl` file is pruned on disk by its harness, any existing digest is **permanently locked** and never overwritten.

### 2.1 Line Naming & Clustering Philosophy (Objective & Context over Brittle Rules)

Rather than constraining the summarizer with brittle regex heuristics or stopword lists (*The Bitter Lesson*), Planar supplies complete world-state context and clear functional objectives:

1. **Role & Granularity Objective**: A Line represents an ongoing, multi-session workstream, project, or focus area (distinct from a single session's task `title` and from `One-off` errands).
2. **Physical UI Affordance**: Prompts explain that Line names render inside a narrow swimlane label column (`148px` desktop, `108px → 78px` in narrow IDE sidebars), encouraging crisp, scannable handles without arbitrary truncation rules.
3. **User Exemplars, Explicit Scope Notes & Merge-to-Teach (`[pinned]` + `description` + Session Counts)**:
   - User-kept (`[pinned]`) Lines act as in-context few-shot exemplars of preferred granularity and language.
   - Each Line supports an optional user-authored **`description` (`Scope:` note)** injected directly into `Sync` and `Regroup` prompts as an authoritative inclusion rule (e.g., telling the model that `search-core` includes `rank-worker` work).
   - **Merge-to-Teach**: Merging Line A into Line B reassigns all sessions, removes Line A, pins Line B, and appends Line A's name/scope into Line B's `description` so future `Sync`/`Regroup` runs preserve the consolidation automatically.
   - `Sync` groups prepared candidates by normalized `cwd` affinity prior to chunking so sessions from the same workspace pack into the same batch while distinct workspaces run concurrently (up to `syncConcurrency`) with minimal cross-batch Line fragmentation; when the catalog has zero non-system Lines, Batch 1 runs as a warm-up seed before fanning out remaining batches, and each newly dispatched batch reads the freshest Line catalog (with active session counts) so related sessions reuse newly discovered Lines verbatim instead of spawning near-duplicate variants.
   - `Regroup` receives `cwd`, `current` Line, `title`, and `stoppedAt` across all unpinned sessions simultaneously so the model distinguishes dedicated project directories from centralized container workspaces empirically.

---

## 3. Active Agent Time Math

For a session's ordered event timestamps and explicit tool execution spans:

1. Clamp each span to `[start_ms, max(start_ms, end_ms)]`.
2. Sort by `(start_ms, end_ms)` and merge adjacent spans whenever `next.start_ms - cur.end_ms <= IDLE_GAP_CAP_MS` (`12 min`).
3. Persist **unpadded** merged intervals in `activity_intervals` so byte-cursor incremental appends remain strictly associative with full scans.
4. Apply the minimum visual/accounting floor (`MIN_BURST_MS = 60s`) only during day projection and range summation.
5. **Dual Line Accounting**:
   - **`agent time`**: Sum of active burst durations across sessions in the Line (reflects total parallel agent compute/attention invested).
   - **`wall clock`**: Interval union across sessions in the Line (physical elapsed span without double-counting overlap).

---

## 4. Visual & Brand Specification

### 4.1 Aesthetic Ethos ("Precision Instrument × Calm Editorial Plane")

1. **Achromatic Canvas, Color Strictly as Semantic Data**:
   - Neutral surfaces (`#FFFFFF` / `#F7F8FA` / `#ECEEF2` in Light; `#121418` / `#181B22` / `#232833` in Dark) with `IBM Plex Sans` + tabular numerals (`font-variant-numeric: tabular-nums`).
   - Color is rationed exclusively for data identity and live state: the 6 OKLab-validated Line hues, the geometric brand mark, subtle harness accents, and single-purpose status signals (`#DE4E18` urgent priority, `#3B66D4` active time cursor).
2. **Attention-Budget Restraint (Quiet Until Asked)**:
   - Default view caps visible Lines at `8` (expanding in-place on demand), folds archived sessions behind `N archived` without discarding metrics, and hides prompt markers / lane guides until hover.
3. **Direct Manipulation & Zero-Jitter Stability**:
   - Expanding Lines, toggling `RECAP` cards, switching metric lenses, or merging Lines preserves scroll position with zero horizontal or vertical jump (`scrollbar-gutter: stable`, synchronous pre-render plot measurement, width-only `ResizeObserver`, `window.scrollY` restoration).
4. **Honest Telemetry Lenses**:
   - `Agent time`, `Tokens`, and `Cost` live in dedicated lenses; partial cost coverage (when a harness transcript omits `model` metadata) is explicitly marked (`*` + coverage tooltip) rather than fabricating USD estimates.

### 4.2 Iconography & Geometric Brand Mark (`24 × 24` Grid)

- **UI Control Symbols (`14 × 14` on `16 × 16` grid, monochrome `currentColor` SVG)**:
  - Drawn with crisp `1.25–1.35 px` strokes paired with one selective solid-ink accent: Hourglass (*Agent time*), Asymmetric BPE Token Chunks (*Tokens*), Sawtooth Receipt (*Cost*), Ascending 3-Step Staircase (*Priority*), and Concentric Ring + MD3 Morphing Core (*Live running session*).
- **Planar App Icon (`assets/icon.svg`, `24 × 24`, `rx = 4`)**:
  - Strict `1/6` (`4`-unit) subdivisions so horizontal boundaries at `y = 4` and `y = 20` meet exact corner tangents:
  - **Top-left Warm Amber (`#F27D16`)**: `y = 0..4` on `[16, 24]`, quarter-circle arc `R = 8` centered at `(8, 4)` from `(16, 4)` to `(8, 12)`, horizontal `y = 12` on `[0, 8]`.
  - **Middle Electric Ultramarine (`#2945F5`)**: Bounded above by the `R = 8` arc and below by `y = 20` on `[0, 8]`, `45°` chamfer `(8, 20) → (16, 12)`, and `y = 12` on `[16, 24]`.
  - **Bottom-right Vivid Rose (`#E63B7A`)**: Fills the lower-right plane.

### 4.3 Categorical Line Palette (OKLab + CVD Validated)

- **Light surface (`#FFFFFF`)**: `#A43D74`, `#2E8540`, `#784BA8`, `#B84A32`, `#008778`, `#2656C4` (lightness `0.43–0.77`, chroma `>= 0.10`, protan/deutan adjacent `ΔE >= 8.1`, normal-vision floor `ΔE >= 20.6`, contrast `>= 3:1`).
- **Dark surface (`#121418`)**: `#D8659D`, `#3BA755`, `#9B72CF`, `#D9684E`, `#1CA99A`, `#4B7BEC`.

### 4.4 Continuous Thickness-Encoded Timeline Bands (`bandProfile` + `bandPath`)

Concurrency is encoded **strictly by band thickness, never by shade or opacity** (`fill: var(--hue)` solid on parent lanes; `opacity: 0.78` only on indented session sub-lanes):

1. **Step Half-Thickness Profile (`bandProfile`)**: Sweeps active intervals across sessions in a lane and maps `0 / 1 / 2 / 3+` concurrent sessions to half-thicknesses `0 / 2.5 / 3.5 / 4.5 px` (`2.0 px` on child sub-lanes). Zero-count gaps narrower than `BAND_JOIN_PX = 2.5 px` are bridged in pixel space to eliminate pinched waists while preserving fine breaks in `Day` view.
2. **Closed-Form Triangular-Window Smoothing (`bandPath`)**: Convolves the step function with a symmetric triangular kernel of half-width `BAND_EASE_PX = 5 px` in closed form via the double integral $G(x)$:
   $$h(x) = \frac{G(x + E) - 2G(x) + G(x - E)}{E^2}$$
   yielding $C^1$ slope-continuous piecewise quadratics ("gentle swells") without per-pixel brute-force summation.
3. **Adaptive Sampling & Capsule Caps**: Samples every `1 px` only inside transition windows $[x_k - E, x_k + E]$, emits only 2 endpoints across flat spans, and closes both ends at full thickness with semicircular SVG `A` arcs.

### 4.5 Signature Plane & List Micro-Interactions

- **Flush Vertical Axis**: Root session status rings sit in the exact same `14 × 14` column as their parent Line swatch (zero extra left indent; only dependent child sessions indent with an `└─` connector).
- **MD3 Expressive Running Indicator**: Live sessions animate their inner dot through $C^1$-continuous 8-knot cubic Bézier shapes (`cookie → pill → clover → squircle`) inside a breathing ring.
- **Hover-Only Prompt Dots (`Day` View)**: User prompt markers (`r ≈ 1.55 px` with 2-tier anti-collision stacking) and horizontal lane guides appear only on hover.
- **Inline `RECAP` Card & Drag-and-Drop Tree**: Clicking any session reveals `Stopped at`, optional `Left open`, `Line` switcher, searchable `+ Link upstream session...` popover, raw initial prompt + step/token/cost footer, `Hide`, and `Open ↗`; dragging Session B onto Session A nests B under A (`↳ Depends on "..."`), and dragging away unlinks (`← Unlink dependency`).
- **Container-Responsive Sidebar (`>= 300px`)**: CSS `@container` queries adapt label columns (`148px → 108px → 78px`), axis tick density, and title ellipsis cleanly down to narrow IDE sidebars (`?sidebar=350`).
