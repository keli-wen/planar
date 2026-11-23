# Zero-Fork Vanilla UI Contract (`./ui`)

> Frontend engineering contract for `ui/index.html`, `ui/styles.css`, and `ui/app.js`, including why Planar deliberately skips React and frontend bundlers.

---

## 1. Why Vanilla Over React / Bundlers

Planar renders one unified UI across four surfaces: `Planar.app` (~15 MB Tauri v2 bundle), `planar serve` (embedded directly in Rust via `include_str!`), zero-backend `npm run preview` (`?mock=1`), and narrow IDE Webview sidebars (`320px–460px`).

- **Zero build-sync friction**: `cargo build` embeds `./ui` directly with no Vite/Webpack `dist/` pipeline or transitive npm bloat (`@biomejs/biome` and `@tauri-apps/cli` are dev-only).
- **Instrument UI fit**: The heaviest UI work is deterministic SVG path math (`bandProfile` + `bandPath`) rather than complex SPA state trees, so three vanilla files keep edits fast for both humans and coding agents.
- **No React skill needed**: Do not add a React best-practice skill or introduce React/Vue/Svelte/Tailwind dependencies. Revisit only if Planar ever grows multi-view collaborative editors that native browser ES modules cannot cleanly sustain.

---

## 2. Hard Boundaries

1. **Zero runtime npm dependencies & zero build step**: `./ui` must run directly from disk (`npm run preview`, `planar serve`) and via `include_str!` inside `crates/planar-core/src/server.rs` without compilation.
2. **No framework drift**: Keep DOM updates deterministic via pure template render functions + delegated event listeners in `ui/app.js`.
3. **Standard HTTP `/api/*` only**: Never hard-couple core data fetches or mutations to `window.__TAURI__`. The identical frontend must work in Tauri v2, `planar serve`, `npm run preview` (`?mock=1`), and IDE Webview sidebars.
4. **Mock-mode parity (`?mock=1`)**: Every new UI feature must gracefully render in mock mode (`?mock=1`) with clean-room synthetic data so agents and contributors can preview and screenshot UI changes without host transcripts.

---

## 3. Layout, Performance & Visual Guardrails

- **Zero-jitter expand/collapse**: Preserve `scrollbar-gutter: stable`, synchronous `computePlotWidthSync()` before SVG generation, width-only `ResizeObserver`, and `window.scrollY` restoration across re-renders.
- **Container-responsive sidebar (`>= 300px`)**: Use CSS `@container` breakpoints (`<= 620px`, `<= 430px`) and test with `?mock=1&sidebar=350`.
- **HTML5 Drag-and-Drop**: Maintain native `draggable="true"` session dependency linking and Line merge drag-and-drop (relying on `"dragDropEnabled": false` in `src-tauri/tauri.conf.json`).
- **Dual Theme & Locale (`zh` / `en`, `system` / `light` / `dark`)**: Every user-visible string belongs in `I18N.zh` and `I18N.en`; categorical Line hues map between `PALETTE` (Light) and `DARK_PALETTE` (Dark) via `resolveLineColor()`.
- **Static Verification**: Run `npm run lint:ui` (`biome lint ui scripts && node --check ...`) after every edit.
