# Planar — Design Principles (`DESIGN.md`)

> Authoritative visual and interaction principles for **Planar** (*"A shared map of the work you do with agents"*). Keep this document concise and principle-driven; live CSS tokens and exact geometry live in [`ui/styles.css`](ui/styles.css), [`ui/app.js`](ui/app.js), and [`assets/icon.svg`](assets/icon.svg).

## 1. Aesthetic North Star

**Precision Instrument × Calm Editorial Plane.** Planar presents parallel agent work as a quiet two-dimensional map (`Lines × Time`), never a noisy telemetry dashboard.

- **Achromatic Canvas, Chromatic Data**: Keep surfaces neutral (`#FFFFFF` Light / `#121418` Dark) structured by `1px` hairlines. Reserve color strictly for Line identity, the 3-plane brand mark, and high-signal status accents (urgent priority, active time cursor, paused/ready dots).
- **Monochrome Chrome**: Primary actions (including `Sync`) and main-plane harness icons stay monochrome so 100% of color bandwidth belongs to workstreams. Full-color harness marks appear only in Settings / engine pickers.
- **Quiet Until Asked**: Cap default visible Lines, fold archived sessions without dropping historical totals, and reveal secondary guides, hover prompt dots, and row action menus only on intent.
- **Spatial Calm**: Expand/collapse, theme flips, and lens switches must never cause horizontal scrollbar shift or vertical scroll jump.

## 2. Brand Mark & Iconography

- **App Icon ([`assets/icon.svg`](assets/icon.svg))**: `24×24` rounded square built from three stroke-free geometric planes — warm amber (`#F27D16`), royal ultramarine (`#2945F5`), and rose-magenta (`#E63B7A`) — aligned to clean `1/6` subdivisions. Always reuse this exact vector mark; never redraw approximations.
- **Control & Status Glyphs (`ui/app.js`)**: Uniform `1.2–1.35px` stroke monochrome icons on crisp `14×14` / `16×16` grids.
  - **Priority**: 3-step ascending staircase (`Urgent` gets the sole warm-vermilion badge; `High`–`None` step cleanly across lit/unlit treads).
  - **Session Activity**: Hollow ring (`Short`), concentric ring + dot (`Idle`), and breathing ring + smooth morphing core (`Running`, respecting `prefers-reduced-motion`).

## 3. Color, Theme & Typography

- **Dual-Theme Parity (`ui/styles.css`)**: Support `system` / `light` / `dark` via CSS custom properties only — never hardcode one-off surface or ink colors in component styles or inline JS.
- **Categorical Line Palette (`PALETTE` ↔ `DARK_PALETTE` in `ui/app.js`)**: Stick to the 6 OKLab/CVD-validated categorical hues and always map through `resolveLineColor()` so Light and Dark themes preserve equal perceptual weight and contrast.
- **Typography & Tabular Numerics**: Pair `IBM Plex Sans` (UI prose & bilingual `zh`/`en` labels) with `IBM Plex Mono` (paths, IDs, timestamps, code). Apply `font-variant-numeric: tabular-nums` to all durations, token counts, costs, and axis labels so right-aligned columns stay rock-steady.
- **Flush Vertical Axis**: Root session icons align flush with parent Line swatches on one vertical axis; only dependent child sessions (`└─`) indent.

## 4. Data Visualization (`Lines × Time`)

- **Thickness Encodes Concurrency, Hue Encodes Identity**: Parent timeline bands use one solid Line hue with smooth $C^1$ variable-thickness swells (`bandProfile` / `bandPath`). Never encode concurrency with opacity gradients, dashed fills, or stacked rainbow bars.
- **Honest, Separated Lenses**: `Time`, `Tokens`, and `Cost` live in strictly separate metric lenses. Distinguish `agent time` (sum across sessions) from `wall clock` (interval union), mark partial coverage with `*`, and show `—` rather than fabricated estimates.
- **Progressive Detail**: Keep the Week/Month plane calm at rest; show per-prompt dots and Session → Plane spotlighting on hover, and open details inline via the Session `RECAP` card without modal popups.

## 5. Layout, Surfaces & Interaction

- **Zero-Fork Multi-Surface Parity**: `./ui` uses zero runtime frameworks and zero build steps, serving `Planar.app`, `planar serve`, `npm run preview` (`?mock=1`), and narrow `320px` IDE sidebars from one codebase via CSS `@container` breakpoints (see [`contexts/engineering/ui-zero-fork.md`](contexts/engineering/ui-zero-fork.md)).
- **Direct Manipulation**: Preserve native HTML5 drag-and-drop for dependency nesting and Line merging (`"dragDropEnabled": false` in `src-tauri/tauri.conf.json`), pointer-draggable 3-Zone Head–Middle–Tail slicer handles, and hidden zero-width scrollbars paired with subtle edge feathering.

## 6. Anti-Patterns & Design QA Checklist

**Never introduce:**
- Neon glows, heavy card drop shadows, glassmorphism blur walls, or saturated gradient chrome.
- Rainbow status pills or colored harness badges on the main timeline plane.
- Modal dialogs where inline expansion (`RECAP`, popovers, in-place settings) works cleanly.
- React/bundler dependencies or Tauri-only IPC for core state.

**Verify before shipping any visual change:**
1. Both **Light** and **Dark** themes (`?mock=1&theme=light` and `&theme=dark`).
2. Both **`zh`** and **`en`** locales with zero overflow or missing `I18N` keys.
3. Narrow sidebar layout (`?mock=1&sidebar=350`) with zero clipped controls.
4. Zero layout/scroll jitter on Line expand/collapse and lens switches.
5. `npm run lint:ui && npm run check:hygiene` clean with zero synthetic privacy leaks.
