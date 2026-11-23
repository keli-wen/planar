# Reference: Adding a New Harness Ingestor or API Provider

> Review Tier: **T2** for a new harness ingestor (touches `ingest/`, `db.rs`, `processor.rs`, `env.rs`, `ui/app.js`); **T1** for a new OpenAI-compatible API provider.

---

## Recipe A — Adding a New Coding-Agent Harness Ingestor

1. **Environment & Resume Helpers (`crates/planar-core/src/env.rs`)**:
   - Add default log root field + `PLANAR_<HARNESS>_DIR` override in `HarnessRoots::from_env_or_home()` and `effective_for()`.
   - Register CLI binary name in `cli_binary_name_for_harness()` and resume command in `build_resume_command()`.
2. **Settings Model (`crates/planar-core/src/model.rs`)**:
   - Add boolean toggle in `EnabledHarnesses` (default `true`) and path override in `HarnessPathOverrides`.
3. **Read-Only Incremental Scanner (`crates/planar-core/src/ingest/<harness>.rs`)**:
   - Stream lines via `ingest::stream_jsonl_lines` + `decide_scan` (`Unchanged` / `AppendFrom` / `Full`).
   - Use `serde_json::value::RawValue` or selective fields to skip heavy tool output payloads without allocating them.
   - Ignore synthetic wrapper blocks, tool-result-only user events, and `[PLANAR_INTERNAL_DIGEST]` echoes.
   - Extract `(model, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens, total_tokens)` accurately without double-counting streaming chunks.
   - Wire into `ingest/mod.rs` (`scan_all_harnesses`) and `db.rs` (`collect_harness_statuses`).
4. **Slicer & Optional CLI Summarizer (`crates/planar-core/src/processor.rs`)**:
   - Add `extract_<harness>_step` in `extract_step_text`.
   - If the harness CLI supports stateless read-only print mode, add it to `invoke_single_cli` (never hardcode unreleased/internal model codenames!).
5. **Frontend (`ui/app.js`)**:
   - Add entry to `HARNESS_ORDER`, `HARNESS_META` (monochrome `24×24` SVG path), and `DEFAULT_MOCK_HARNESSES`.
6. **Tests & Context Update**:
   - Add offline `TempDir` unit tests covering full scan, byte-cursor incremental append, synthetic-wrapper filtering, subagent handling, and token accounting.
   - Update `contexts/project/architecture.md` and `skills/planar/SKILL.md` if user-facing CLI/MCP support expanded.
   - Run `npm run verify`.

---

## Recipe B — Adding a New OpenAI-Compatible API Provider

1. Add one `OpenAiCompatSpec` entry in `crates/planar-core/src/processor.rs` (`openai_compat_spec`) with `default_base`, `default_model`, `env_vars`, and `allow_keyless`.
2. Add the matching provider entry to `API_PROVIDERS` in `ui/app.js`.
3. Add a unit assertion in `crates/planar-core/src/processor.rs` (`#[cfg(test)] mod tests`).
4. Update `contexts/project/architecture.md` (§2.2) and run `npm run verify`.
