# Adding a New Harness Ingestor or API Provider

> Engineering runbook for extending Planar's read-only trajectory ingestors and direct LLM API providers. Review Tier per [`REVIEW.md`](../../REVIEW.md): **T2** for a new harness ingestor (touches `ingest/`, `db.rs`, `processor.rs`, `env.rs`, `defaults.rs`, `ui/defaults.json`, `ui/app.js`); **T1** for a new OpenAI-compatible API provider.

---

## Recipe A — Adding a New Coding-Agent Harness Ingestor

1. **Environment & Resume Helpers (`crates/planar-core/src/env.rs`)**:
   - Add default log root field + `PLANAR_<HARNESS>_DIR` override in `HarnessRoots::from_env_or_home()`, `effective_for()`, and `primary_root_for()`.
   - Register CLI binary name in `cli_binary_name_for_harness()` and resume command in `build_resume_command()`.
2. **Shared Defaults & Settings Model (`ui/defaults.json`, `crates/planar-core/src/defaults.rs`, `crates/planar-core/src/model.rs`)**:
   - Add the harness ID to `harnessOrder` (and `cliEngineOrder` if CLI-capable) plus `defaultSettings.enabledHarnesses` (default `true`) and `defaultSettings.harnessPaths` (`""`) in `ui/defaults.json`.
   - Mirror the new fields in `defaults.rs` (`EnabledHarnessesDef`, `HarnessPathOverridesDef`, `default_enabled_harnesses()`, `default_harness_paths()`) and `model.rs` (`EnabledHarnesses`, `EnabledHarnesses::is_enabled()`, `HarnessPathOverrides`).
   - Update `defaults_json_orders_and_thresholds_are_consistent` in `crates/planar-core/src/defaults.rs` for the new harness count (and for a scan-only harness omitted from `cliEngineOrder`, assert `cli_order.is_subset(&harnesses)` instead of set equality).
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
   - Add entry to `HARNESS_META` (monochrome `24×24` SVG path) and `DEFAULT_MOCK_HARNESSES` (`HARNESS_ORDER` is loaded directly from `ui/defaults.json`).
6. **Tests & Context Update**:
   - Add offline `TempDir` unit tests covering full scan, byte-cursor incremental append, synthetic-wrapper filtering, subagent handling, and token accounting.
   - Update [`contexts/project/architecture.md`](../project/architecture.md) and [`skills/planar/SKILL.md`](../../skills/planar/SKILL.md) if user-facing CLI/MCP support expanded.
   - Run `npm run verify`.

---

## Recipe B — Adding a New OpenAI-Compatible API Provider

Planar stores its direct-API provider catalog in a single declarative source of truth (`ui/defaults.json`), embedded into Rust via `crates/planar-core/src/defaults.rs` (`ApiProviderSpec` / `api_provider()`) and loaded directly by `ui/app.js` (`SHARED_DEFAULTS.apiProviders`). Any provider other than `gemini` and `anthropic` is automatically routed through `call_openai_compat_api` in `crates/planar-core/src/processor.rs`.

1. **Register in SSOT (`ui/defaults.json`)**:
   - Add one object to `apiProviders` with lowercase `id`, `labelZh`, `labelEn`, `defaultBase`, `defaultModel`, `envVars`, `allowKeyless`, and optional `requiresBaseUrl: true`.
2. **Add Offline Unit Assertions**:
   - Add the new provider ID to `defaults_json_provider_catalog_has_non_empty_unique_fields` in `crates/planar-core/src/defaults.rs` (verifies schema, unique ID, non-empty bilingual labels, base URL, default model, and env vars).
   - If the provider introduces custom endpoint/key/billing behavior, also add targeted tests in `crates/planar-core/src/processor.rs` (`#[cfg(test)] mod tests`) and/or `scripts/test-ui-defaults.mjs`.
3. **Update Context & Verify**:
   - Update [`contexts/project/architecture.md`](../project/architecture.md) (§2.2) and run `npm run verify`.
