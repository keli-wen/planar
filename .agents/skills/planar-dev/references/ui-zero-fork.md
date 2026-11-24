# Reference: Zero-Fork Vanilla UI (`./ui`) Workflow

> Source of truth: `DESIGN.md` (mandatory for all tokens, geometry & visual QA), `contexts/engineering/ui-zero-fork.md`, and `contexts/project/design.md §4`.

## Core Rules When Editing `./ui`

1. **Stay 100% vanilla HTML/CSS/ESM** — zero React, zero JSX, zero npm runtime packages, zero build pipeline. Everything in `ui/index.html`, `ui/styles.css`, and `ui/app.js` is embedded verbatim into Rust via `include_str!`.
2. **Keep HTTP-only state flow** — mutate via `apiApply('/api/...', method, body)` or streaming `fetch('/api/sync/stream?...')`. Never introduce Tauri-only IPC for core state.
3. **Maintain mock-mode parity (`?mock=1`)** — ensure new controls and views work cleanly when `state.dataSource === "mock"` using clean-room synthetic data only (zero personal project names, paths, or transcripts).
4. **Keep bilingual & dual-theme parity** — add keys to both `I18N.zh` and `I18N.en`; verify both Light (`#FFFFFF`) and Dark (`#121418`) themes and categorical palette mapping (`PALETTE` / `DARK_PALETTE`).
5. **Preserve zero-jitter & narrow sidebar responsiveness** — never break `scrollbar-gutter: stable`, `computePlotWidthSync()`, width-only `ResizeObserver`, or `@container` breakpoints (`<= 620px`, `<= 430px`).

## Local Preview & Verification

```bash
npm run lint:ui       # Biome lint on ui/ + scripts/ and node --check syntax validation
npm run preview       # Starts static server at http://127.0.0.1:4310/ui/
```

Test these preview URLs before finalizing visual edits:
- Week + dependency tree: `http://127.0.0.1:4310/ui/?mock=1&lang=en&theme=light&mode=week&expand=1&dep=1`
- Day + inline `RECAP` card: `http://127.0.0.1:4310/ui/?mock=1&lang=en&theme=light&mode=day&day=4&hover=planar&recap=1`
- Dark mode: append `&theme=dark`
- Narrow sidebar (`350px`): append `&sidebar=350`
- Settings slicer: `http://127.0.0.1:4310/ui/?mock=1&lang=en&theme=light&view=settings&tab=lines`

## Refreshing README Screenshots (`assets/ui-*.png`)

Whenever major UI chrome changes, regenerate clean-room 2x Retina screenshots from `?mock=1` using Headless Chrome (never capture real user transcripts!):

```bash
PORT=4319 node ./scripts/preview.mjs >/tmp/planar-preview.log 2>&1 &
PID=$!; trap "kill $PID 2>/dev/null || true" EXIT; sleep 0.5
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 --window-size=920,760 --virtual-time-budget=3000 --screenshot="$PWD/assets/ui-week.png" "http://127.0.0.1:4319/ui/?mock=1&lang=en&theme=light&mode=week&expand=1&dep=1"
"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 --window-size=920,960 --virtual-time-budget=3000 --screenshot="$PWD/assets/ui-day.png" "http://127.0.0.1:4319/ui/?mock=1&lang=en&theme=light&mode=day&day=4&hover=planar&recap=1"
"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 --window-size=920,890 --virtual-time-budget=3000 --screenshot="$PWD/assets/ui-settings.png" "http://127.0.0.1:4319/ui/?mock=1&lang=en&theme=light&view=settings&tab=lines"
rm -f /tmp/planar-preview.log
```

Inspect every captured PNG visually before committing.
