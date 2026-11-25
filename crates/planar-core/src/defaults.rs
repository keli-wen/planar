//! Single source of truth for Planar's shared declarative catalog (`ui/defaults.json`)
//! and Rust-internal engine tuning constants and validation bounds.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::model::{AppSettings, EnabledHarnesses, HarnessPathOverrides};

/// Raw embedded `ui/defaults.json` shared across `planar-core`, `Planar.app`, `planar serve`,
/// and `npm run preview`.
pub const DEFAULTS_JSON: &str = include_str!("../../../ui/defaults.json");

/// Expected number of categorical hues in both Light and Dark palettes.
pub const EXPECTED_PALETTE_LEN: usize = 6;
/// Palette index assigned to the pinned `One-off` system bucket line.
pub const ONE_OFF_LINE_COLOR_INDEX: usize = 4;

/// Sentinel marker placed inside internal CLI digest prompts so scanners never re-ingest them.
pub const INTERNAL_DIGEST_SENTINEL: &str = "[PLANAR_INTERNAL_DIGEST]";
/// Reserved slug for the pinned `One-off` system bucket line.
pub const ONE_OFF_LINE_ID: &str = "one-off";

/// Maximum idle gap between consecutive events merged into one active burst (`12 min`).
pub const IDLE_GAP_CAP_MS: i64 = 12 * 60 * 1000;
/// Minimum visual and accounting floor applied when projecting stored bursts onto days (`1 min`).
pub const MIN_BURST_MS: i64 = 60 * 1000;

/// Minimum number of sessions allowed per `Sync` batch.
pub const SYNC_BATCH_LIMIT_MIN: usize = 1;
/// Maximum number of sessions allowed per `Sync` batch.
pub const SYNC_BATCH_LIMIT_MAX: usize = 50;

/// Minimum user inputs threshold for multi-turn summarization.
pub const FEWEST_USER_INPUTS_MIN: u32 = 1;
/// Minimum opening/closing step count retained by the Head–Middle–Tail slicer.
pub const SLICER_STEPS_MIN: u32 = 5;
/// Minimum character truncation floor applied by the Head–Middle–Tail slicer.
pub const SLICER_CHAR_BUDGET_MIN: usize = 100;
/// Minimum number of sessions displayed per Line before folding.
pub const SESSIONS_SHOWN_PER_LINE_MIN: usize = 1;

/// Minimum timeline window span in days.
pub const WINDOW_DAYS_MIN: usize = 1;
/// Maximum timeline window span in days (`Day`/`Week`/`Month` views & `Sync` scope).
pub const WINDOW_DAYS_MAX: usize = 31;
/// Maximum window span in days for `planar status` L0 snapshots.
pub const STATUS_WINDOW_DAYS_MAX: usize = 30;
/// Maximum lookback span in days for `planar search`.
pub const SEARCH_DAYS_MAX: usize = 3650;

/// Minimum result limit for L1 line sessions and FTS5 search queries.
pub const QUERY_LIMIT_MIN: usize = 1;
/// Maximum result limit for L1 line sessions and FTS5 search queries.
pub const QUERY_LIMIT_MAX: usize = 50;
/// Maximum number of Lines accepted in a single L1 `query_lines` batch.
pub const MAX_L1_BATCH_LINES: usize = 8;
/// Maximum number of Sessions accepted in a single L2 `query_sessions` batch.
pub const MAX_L2_BATCH_SESSIONS: usize = 3;
/// Maximum number of recent short sessions returned in `PlaneResponse`.
pub const MAX_SHORT_ITEMS_RETURNED: usize = 20;

/// Minimum numeric Line priority (`0 = None`).
pub const LINE_PRIORITY_MIN: i64 = 0;
/// Maximum numeric Line priority (`4 = Low`).
pub const LINE_PRIORITY_MAX: i64 = 4;

/// Timeout in seconds for a single local CLI summarizer invocation.
pub const CLI_TIMEOUT_SECS: u64 = 120;
/// Timeout in seconds for a single direct HTTP LLM API request.
pub const API_TIMEOUT_SECS: u64 = 90;
/// Heartbeat interval in seconds for streaming NDJSON `Sync` responses.
pub const STREAM_HEARTBEAT_SECS: u64 = 3;
/// Approximate character ceiling for one multi-session `Sync` batch prompt.
pub const MAX_BATCH_PROMPT_CHARS: usize = 72_000;

/// Cooldown window in milliseconds between automatic background scans on `GET /api/plane`.
pub const AUTO_SCAN_THROTTLE_MS: i64 = 3_000;
/// Maximum character length kept when normalizing a session's `first_user_prompt`.
pub const MAX_PROMPT_CHARS: usize = 240;

/// Declarative metadata for a supported direct LLM API provider loaded from `ui/defaults.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiProviderSpec {
    pub id: String,
    pub label_zh: String,
    pub label_en: String,
    pub default_base: String,
    pub default_model: String,
    pub env_vars: Vec<String>,
    pub allow_keyless: bool,
    #[serde(default)]
    pub requires_base_url: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnabledHarnessesDef {
    antigravity: bool,
    claude: bool,
    codex: bool,
    dsh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HarnessPathOverridesDef {
    antigravity: String,
    claude: String,
    codex: String,
    dsh: String,
}

/// Strict schema for `defaultSettings` inside `ui/defaults.json` (intentionally omits
/// `#[serde(default)]` so missing keys fail deserialization and `AppSettings::default()`
/// never recurses into `SHARED_DEFAULTS`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefaultSettingsDef {
    auto_sync: String,
    fewest_user_inputs: u32,
    show_short_on_timeline: bool,
    include_archived: bool,
    opening_steps: u32,
    closing_steps: u32,
    model: String,
    longest_user_chars: u32,
    longest_agent_chars: u32,
    sessions_shown_per_line: usize,
    language: String,
    theme: String,
    metric_lens: String,
    show_contribution_graph: bool,
    processor_configured: bool,
    processor_mode: String,
    cli_harness: String,
    cli_model: String,
    cli_bin_override: String,
    api_provider: String,
    api_base_url: String,
    api_model: String,
    api_key: String,
    sync_batch_limit: usize,
    enabled_harnesses: EnabledHarnessesDef,
    harness_paths: HarnessPathOverridesDef,
}

/// Declarative defaults parsed once from embedded `ui/defaults.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedDefaults {
    pub palette: Vec<String>,
    pub dark_palette: Vec<String>,
    pub harness_order: Vec<String>,
    pub cli_engine_order: Vec<String>,
    pub short_threshold_options: Vec<u32>,
    default_settings: DefaultSettingsDef,
    pub api_providers: Vec<ApiProviderSpec>,
}

static SHARED_DEFAULTS: LazyLock<SharedDefaults> =
    LazyLock::new(
        || match serde_json::from_str::<SharedDefaults>(DEFAULTS_JSON) {
            Ok(parsed) => parsed,
            Err(err) => panic!("embedded ui/defaults.json is invalid: {err}"),
        },
    );

/// Returns the lazily parsed shared declarative catalog from `ui/defaults.json`.
pub fn shared_defaults() -> &'static SharedDefaults {
    &SHARED_DEFAULTS
}

/// Returns the Light-theme `OKLab` & CVD-validated categorical line palette.
pub fn palette() -> &'static [String] {
    &shared_defaults().palette
}

/// Returns the Dark-theme (`#121418`) `OKLab` & CVD-validated categorical line palette.
pub fn dark_palette() -> &'static [String] {
    &shared_defaults().dark_palette
}

/// Returns the Light-theme categorical palette color at `index` (wrapping around palette length).
pub fn palette_color(index: usize) -> &'static str {
    let colors = palette();
    if colors.is_empty() {
        return "#7E828A";
    }
    colors[index % colors.len()].as_str()
}

/// Returns the canonical display order for trajectory harnesses.
pub fn harness_order() -> &'static [String] {
    &shared_defaults().harness_order
}

/// Returns the CLI selector & `auto` fallback priority (`claude` placed last due to strict quota).
pub fn cli_engine_order() -> &'static [String] {
    &shared_defaults().cli_engine_order
}

/// Returns the selectable short-session user-input threshold presets.
pub fn short_threshold_options() -> &'static [u32] {
    &shared_defaults().short_threshold_options
}

/// Returns the full declarative direct-API provider catalog.
pub fn api_providers() -> &'static [ApiProviderSpec] {
    &shared_defaults().api_providers
}

/// Looks up one direct-API provider definition by canonical lowercase `id`.
pub fn api_provider(id: &str) -> Option<&'static ApiProviderSpec> {
    api_providers()
        .iter()
        .find(|spec| spec.id.eq_ignore_ascii_case(id))
}

/// Resolves the catalog `defaultModel` for `provider_id` from `ui/defaults.json`.
pub fn default_api_model_for_provider(provider_id: &str) -> String {
    api_provider(provider_id).map_or_default(|spec| spec.default_model.clone())
}

/// Constructs default `EnabledHarnesses` from `ui/defaults.json`.
pub fn default_enabled_harnesses() -> EnabledHarnesses {
    let src = &shared_defaults().default_settings.enabled_harnesses;
    EnabledHarnesses {
        antigravity: src.antigravity,
        claude: src.claude,
        codex: src.codex,
        dsh: src.dsh,
    }
}

/// Constructs default `HarnessPathOverrides` from `ui/defaults.json`.
pub fn default_harness_paths() -> HarnessPathOverrides {
    let src = &shared_defaults().default_settings.harness_paths;
    HarnessPathOverrides {
        antigravity: src.antigravity.clone(),
        claude: src.claude.clone(),
        codex: src.codex.clone(),
        dsh: src.dsh.clone(),
    }
}

/// Constructs default `AppSettings` directly from `ui/defaults.json`.
pub fn default_settings() -> AppSettings {
    let src = &shared_defaults().default_settings;
    AppSettings {
        auto_sync: src.auto_sync.clone(),
        fewest_user_inputs: src.fewest_user_inputs,
        show_short_on_timeline: src.show_short_on_timeline,
        include_archived: src.include_archived,
        opening_steps: src.opening_steps,
        closing_steps: src.closing_steps,
        model: src.model.clone(),
        longest_user_chars: src.longest_user_chars,
        longest_agent_chars: src.longest_agent_chars,
        sessions_shown_per_line: src.sessions_shown_per_line,
        language: src.language.clone(),
        theme: src.theme.clone(),
        metric_lens: src.metric_lens.clone(),
        show_contribution_graph: src.show_contribution_graph,
        processor_configured: src.processor_configured,
        processor_mode: src.processor_mode.clone(),
        cli_harness: src.cli_harness.clone(),
        cli_model: src.cli_model.clone(),
        cli_bin_override: src.cli_bin_override.clone(),
        api_provider: src.api_provider.clone(),
        api_base_url: src.api_base_url.clone(),
        api_model: default_api_model_for_provider(&src.api_provider),
        api_key: src.api_key.clone(),
        sync_batch_limit: src.sync_batch_limit,
        enabled_harnesses: default_enabled_harnesses(),
        harness_paths: default_harness_paths(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashSet;

    use serde_json::Value;

    use super::*;

    fn is_hex_color(raw: &str) -> bool {
        raw.len() == 7 && raw.starts_with('#') && raw[1..].bytes().all(|b| b.is_ascii_hexdigit())
    }

    #[test]
    fn defaults_json_deserializes_and_palette_has_light_dark_parity() {
        let parsed: SharedDefaults = serde_json::from_str(DEFAULTS_JSON).unwrap();
        assert_eq!(parsed.palette.len(), EXPECTED_PALETTE_LEN);
        assert_eq!(parsed.dark_palette.len(), EXPECTED_PALETTE_LEN);

        let light_unique: HashSet<&str> = parsed.palette.iter().map(String::as_str).collect();
        let dark_unique: HashSet<&str> = parsed.dark_palette.iter().map(String::as_str).collect();
        assert_eq!(light_unique.len(), EXPECTED_PALETTE_LEN);
        assert_eq!(dark_unique.len(), EXPECTED_PALETTE_LEN);

        for hex in parsed.palette.iter().chain(parsed.dark_palette.iter()) {
            assert!(is_hex_color(hex), "invalid hex palette color `{hex}`");
        }
        assert_eq!(
            palette_color(ONE_OFF_LINE_COLOR_INDEX),
            parsed.palette[ONE_OFF_LINE_COLOR_INDEX]
        );
    }

    #[test]
    fn defaults_json_provider_catalog_has_non_empty_unique_fields() {
        let providers = api_providers();
        assert_ne!(providers, []);

        let mut seen_ids = HashSet::new();
        for p in providers {
            assert!(!p.id.trim().is_empty(), "provider id must be non-empty");
            assert_eq!(
                p.id,
                p.id.to_ascii_lowercase(),
                "provider id must be lowercase"
            );
            assert!(
                seen_ids.insert(p.id.as_str()),
                "duplicate provider `{}`",
                p.id
            );
            assert!(
                !p.label_zh.trim().is_empty(),
                "empty labelZh for `{}`",
                p.id
            );
            assert!(
                !p.label_en.trim().is_empty(),
                "empty labelEn for `{}`",
                p.id
            );
            assert!(
                !p.default_base.trim().is_empty(),
                "empty defaultBase for `{}`",
                p.id
            );
            assert!(
                !p.default_model.trim().is_empty(),
                "empty defaultModel for `{}`",
                p.id
            );
            if !p.allow_keyless {
                assert!(
                    !p.env_vars.is_empty(),
                    "non-keyless provider `{}` must declare envVars",
                    p.id
                );
            }
            for var in &p.env_vars {
                assert!(
                    !var.trim().is_empty(),
                    "blank envVar entry for provider `{}`",
                    p.id
                );
            }
        }

        for required in [
            "gemini",
            "openai",
            "anthropic",
            "deepseek",
            "openrouter",
            "xai",
            "groq",
            "mistral",
            "siliconflow",
            "moonshot",
            "ollama",
            "openai-compatible",
        ] {
            assert!(
                api_provider(required).is_some(),
                "missing required API provider `{required}`"
            );
        }
        assert!(api_provider("ollama").unwrap().allow_keyless);
        assert!(api_provider("openai-compatible").unwrap().requires_base_url);
        assert_eq!(
            api_provider("gemini").unwrap().default_model,
            "gemini-4-hydrogen"
        );
        assert_eq!(
            api_provider("openrouter").unwrap().default_model,
            "google/gemini-4-hydrogen"
        );
    }

    #[test]
    fn defaults_json_orders_and_thresholds_are_consistent() {
        let harnesses: HashSet<&str> = harness_order().iter().map(String::as_str).collect();
        let cli_order: HashSet<&str> = cli_engine_order().iter().map(String::as_str).collect();
        assert_eq!(harness_order().len(), 4);
        assert_eq!(cli_engine_order().len(), 4);
        assert_eq!(harnesses, cli_order);
        assert_eq!(
            cli_engine_order().last().map(String::as_str),
            Some("claude")
        );

        let thresholds = short_threshold_options();
        assert_ne!(thresholds, [0_u32; 0]);
        assert!(thresholds.windows(2).all(|w| w[0] < w[1]));
        assert!(thresholds[0] >= FEWEST_USER_INPUTS_MIN);
        assert!(thresholds.contains(&AppSettings::default().fewest_user_inputs));
    }

    #[test]
    fn app_settings_default_aligns_with_defaults_json_and_bounds() {
        let settings = AppSettings::default();
        let raw_root: Value = serde_json::from_str(DEFAULTS_JSON).unwrap();
        let default_prov = api_provider(&settings.api_provider).unwrap();
        assert_eq!(settings.api_model, default_prov.default_model);
        assert_eq!(
            raw_root["defaultSettings"]["apiModel"],
            Value::String(String::new()),
            "defaultSettings.apiModel should stay empty so provider catalog remains the single source of truth"
        );

        let mut expected = raw_root["defaultSettings"].clone();
        expected["apiModel"] = Value::String(default_prov.default_model.clone());
        let serialized = serde_json::to_value(&settings).unwrap();
        assert_eq!(serialized, expected);

        assert!((SYNC_BATCH_LIMIT_MIN..=SYNC_BATCH_LIMIT_MAX).contains(&settings.sync_batch_limit));
        assert!(settings.fewest_user_inputs >= FEWEST_USER_INPUTS_MIN);
        assert!(settings.opening_steps >= SLICER_STEPS_MIN);
        assert!(settings.closing_steps >= SLICER_STEPS_MIN);
        assert!(settings.sessions_shown_per_line >= SESSIONS_SHOWN_PER_LINE_MIN);
        const {
            assert!(SYNC_BATCH_LIMIT_MIN <= SYNC_BATCH_LIMIT_MAX);
        }
    }

    #[test]
    fn app_settings_deserialization_derives_provider_model_and_preserves_overrides() {
        let empty_obj: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty_obj, AppSettings::default());

        let provider_only: AppSettings =
            serde_json::from_str(r#"{"apiProvider":"deepseek"}"#).unwrap();
        assert_eq!(provider_only.api_provider, "deepseek");
        assert_eq!(
            provider_only.api_model,
            api_provider("deepseek").unwrap().default_model
        );

        let with_override: AppSettings =
            serde_json::from_str(r#"{"apiProvider":"deepseek","apiModel":"deepseek-reasoner"}"#)
                .unwrap();
        assert_eq!(with_override.api_provider, "deepseek");
        assert_eq!(with_override.api_model, "deepseek-reasoner");

        let with_explicit_empty: AppSettings =
            serde_json::from_str(r#"{"apiProvider":"deepseek","apiModel":""}"#).unwrap();
        assert_eq!(with_explicit_empty.api_provider, "deepseek");
        assert_eq!(with_explicit_empty.api_model, "");

        for blank in ["", "   "] {
            let blank_provider: AppSettings = serde_json::from_value(serde_json::json!({
                "processorConfigured": true,
                "processorMode": "api",
                "apiProvider": blank,
            }))
            .unwrap();
            assert_eq!(blank_provider.api_provider, blank);
            assert_eq!(blank_provider.api_model, "");
            assert!(!blank_provider.is_processor_configured());
        }
    }
}
