use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::defaults::{INTERNAL_DIGEST_SENTINEL, ONE_OFF_LINE_ID, dark_palette, palette};

/// Per-harness enablement toggles stored in `AppSettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnabledHarnesses {
    pub antigravity: bool,
    pub claude: bool,
    pub codex: bool,
    pub dsh: bool,
}

impl Default for EnabledHarnesses {
    fn default() -> Self {
        crate::defaults::default_enabled_harnesses()
    }
}

impl EnabledHarnesses {
    /// Returns whether trajectories from `harness` should be scanned and displayed.
    pub fn is_enabled(&self, harness: &str) -> bool {
        match harness {
            "antigravity" => self.antigravity,
            "claude" => self.claude,
            "codex" => self.codex,
            "dsh" => self.dsh,
            _ => true,
        }
    }
}

/// Optional custom log directory paths per harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HarnessPathOverrides {
    pub antigravity: String,
    pub claude: String,
    pub codex: String,
    pub dsh: String,
}

impl Default for HarnessPathOverrides {
    fn default() -> Self {
        crate::defaults::default_harness_paths()
    }
}

const fn default_true() -> bool {
    true
}

/// User settings persisted in SQLite and shared with `./ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "AppSettingsWire")]
pub struct AppSettings {
    pub auto_sync: String,
    pub fewest_user_inputs: u32,
    pub show_short_on_timeline: bool,
    pub include_archived: bool,
    pub opening_steps: u32,
    pub closing_steps: u32,
    pub model: String,
    pub longest_user_chars: u32,
    pub longest_agent_chars: u32,
    pub sessions_shown_per_line: usize,
    pub language: String,
    pub theme: String,
    pub metric_lens: String,
    pub show_contribution_graph: bool,
    pub processor_configured: bool,
    pub processor_mode: String,
    pub cli_harness: String,
    pub cli_model: String,
    pub cli_bin_override: String,
    pub api_provider: String,
    pub api_base_url: String,
    pub api_model: String,
    pub api_key: String,
    pub sync_batch_limit: usize,
    pub enabled_harnesses: EnabledHarnesses,
    pub harness_paths: HarnessPathOverrides,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AppSettingsWire {
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
    api_model: Option<String>,
    api_key: String,
    sync_batch_limit: usize,
    enabled_harnesses: EnabledHarnesses,
    harness_paths: HarnessPathOverrides,
}

impl Default for AppSettingsWire {
    fn default() -> Self {
        let defaults = crate::defaults::default_settings();
        Self {
            auto_sync: defaults.auto_sync,
            fewest_user_inputs: defaults.fewest_user_inputs,
            show_short_on_timeline: defaults.show_short_on_timeline,
            include_archived: defaults.include_archived,
            opening_steps: defaults.opening_steps,
            closing_steps: defaults.closing_steps,
            model: defaults.model,
            longest_user_chars: defaults.longest_user_chars,
            longest_agent_chars: defaults.longest_agent_chars,
            sessions_shown_per_line: defaults.sessions_shown_per_line,
            language: defaults.language,
            theme: defaults.theme,
            metric_lens: defaults.metric_lens,
            show_contribution_graph: defaults.show_contribution_graph,
            processor_configured: defaults.processor_configured,
            processor_mode: defaults.processor_mode,
            cli_harness: defaults.cli_harness,
            cli_model: defaults.cli_model,
            cli_bin_override: defaults.cli_bin_override,
            api_provider: defaults.api_provider,
            api_base_url: defaults.api_base_url,
            api_model: None,
            api_key: defaults.api_key,
            sync_batch_limit: defaults.sync_batch_limit,
            enabled_harnesses: defaults.enabled_harnesses,
            harness_paths: defaults.harness_paths,
        }
    }
}

impl From<AppSettingsWire> for AppSettings {
    fn from(wire: AppSettingsWire) -> Self {
        let api_model = wire
            .api_model
            .unwrap_or_else(|| crate::defaults::default_api_model_for_provider(&wire.api_provider));
        Self {
            auto_sync: wire.auto_sync,
            fewest_user_inputs: wire.fewest_user_inputs,
            show_short_on_timeline: wire.show_short_on_timeline,
            include_archived: wire.include_archived,
            opening_steps: wire.opening_steps,
            closing_steps: wire.closing_steps,
            model: wire.model,
            longest_user_chars: wire.longest_user_chars,
            longest_agent_chars: wire.longest_agent_chars,
            sessions_shown_per_line: wire.sessions_shown_per_line,
            language: wire.language,
            theme: wire.theme,
            metric_lens: wire.metric_lens,
            show_contribution_graph: wire.show_contribution_graph,
            processor_configured: wire.processor_configured,
            processor_mode: wire.processor_mode,
            cli_harness: wire.cli_harness,
            cli_model: wire.cli_model,
            cli_bin_override: wire.cli_bin_override,
            api_provider: wire.api_provider,
            api_base_url: wire.api_base_url,
            api_model,
            api_key: wire.api_key,
            sync_batch_limit: wire.sync_batch_limit,
            enabled_harnesses: wire.enabled_harnesses,
            harness_paths: wire.harness_paths,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        crate::defaults::default_settings()
    }
}

impl AppSettings {
    /// Returns whether the user has explicitly selected and saved a summarization processor.
    pub fn is_processor_configured(&self) -> bool {
        if !self.processor_configured {
            return false;
        }
        if self.processor_mode == "api" {
            !self.api_provider.trim().is_empty()
        } else {
            !self.cli_harness.trim().is_empty()
        }
    }
}

/// Day column metadata returned by `/api/plane` for `Day`, `Week`, and `Month` views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayMeta {
    pub idx: usize,
    pub iso_date: String,
    pub short: String,
    pub tiny: String,
    pub full: String,
    pub is_today: bool,
}

/// Single day cell inside the 20-week GitHub-style Contribution Calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributionDayDto {
    pub date: String,
    pub weekday: u8,
    pub total_minutes: i64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: f64,
    pub session_count: usize,
    pub by_harness: BTreeMap<String, i64>,
    #[serde(default)]
    pub tokens_by_harness: BTreeMap<String, u64>,
    #[serde(default)]
    pub cost_by_harness: BTreeMap<String, f64>,
}

/// Live handler & CLI availability status for one trajectory harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessStatusDto {
    pub id: String,
    pub enabled: bool,
    pub cli_available: bool,
    pub cli_path: Option<String>,
    pub log_root: String,
    pub log_root_exists: bool,
    pub session_count: usize,
}

/// Actual token usage and computed API spend accumulated during a `Sync` or `Regroup` run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncUsageDto {
    /// Uncached input tokens sent to the model.
    pub input_tokens: u64,
    /// Cached input tokens read from prompt cache.
    pub cache_read_tokens: u64,
    /// Prompt-cache creation / write tokens (e.g. Anthropic).
    pub cache_write_tokens: u64,
    /// Visible completion / candidate output tokens (excluding `thinking_tokens`).
    pub output_tokens: u64,
    /// Reasoning / thought tokens (separate from `output_tokens`, billed as output).
    pub thinking_tokens: u64,
    /// Sum of all token buckets above.
    pub total_tokens: u64,
    /// Computed API USD spend via `genai-prices`, or `None` for local CLI / unpriced models.
    /// When `incomplete` is `true`, this figure is a partial subtotal covering only `priced_calls`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Active model identifier used for the run, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `true` when executed via a local CLI subscription (`agy`/`codex`/`dsh`/`claude`) with no
    /// direct per-token API spend.
    pub is_local_cli: bool,
    /// Total number of direct API calls observed in this run or batch.
    pub api_calls: usize,
    /// Number of direct API calls whose usage metadata was present and priced into `cost_usd`.
    pub priced_calls: usize,
    /// `true` when at least one direct API call omitted usage metadata or lacked known billing
    /// pricing, making `total_tokens` and/or `cost_usd` an incomplete subtotal.
    pub incomplete: bool,
}

impl SyncUsageDto {
    /// Creates an honest local-CLI usage record (no fabricated per-token API spend).
    #[must_use]
    pub fn local_cli(model: Option<String>) -> Self {
        Self {
            model: model.filter(|m| !m.trim().is_empty()),
            is_local_cli: true,
            ..Self::default()
        }
    }

    /// Creates a record for a direct API call whose response omitted usage metadata.
    #[must_use]
    pub fn missing_api_usage(model: Option<String>) -> Self {
        Self {
            model: model.filter(|m| !m.trim().is_empty()),
            is_local_cli: false,
            api_calls: 1,
            priced_calls: 0,
            incomplete: true,
            ..Self::default()
        }
    }

    /// Merges another call or batch usage snapshot into `self`, preserving honest partial-coverage
    /// flags whenever any call lacked usage metadata or model pricing.
    pub fn merge_from(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.thinking_tokens = self.thinking_tokens.saturating_add(other.thinking_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if self.model.is_none() && other.model.is_some() {
            self.model.clone_from(&other.model);
        }
        self.api_calls = self.api_calls.saturating_add(other.api_calls);
        self.priced_calls = self.priced_calls.saturating_add(other.priced_calls);
        self.incomplete = self.incomplete
            || other.incomplete
            || (self.api_calls > 0 && self.priced_calls < self.api_calls);
        if other.is_local_cli
            && self.api_calls == 0
            && self.total_tokens == 0
            && self.cost_usd.is_none()
        {
            self.is_local_cli = true;
        } else if !other.is_local_cli {
            self.is_local_cli = false;
        }
    }
}

/// Low/high USD cost range for a pre-flight `Sync` estimate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostUsdRangeDto {
    pub low: f64,
    pub high: f64,
}

/// Read-only pre-flight cost and token estimate returned by `GET /api/sync/estimate`
/// and `planar sync --estimate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEstimateDto {
    pub session_count: usize,
    pub batch_count: usize,
    pub estimated_input_tokens: u64,
    pub estimated_output_tokens: u64,
    #[serde(default)]
    pub estimated_thinking_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd_range: Option<CostUsdRangeDto>,
    pub model: String,
    pub backend: String,
    pub is_local_cli: bool,
}

/// Outcome summary attached to `/api/sync` and `/api/regroup` responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummaryDto {
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub backend_used: String,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SyncUsageDto>,
}

/// Preview metadata for one session inside an active `Sync` batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatchPreviewItem {
    pub session_id: String,
    pub harness: String,
    pub label: String,
}

/// Filed digest preview inside a completed `Sync` batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFiledPreview {
    pub session_id: String,
    pub title: String,
    pub line_name: String,
}

/// Payload emitted when a multi-session `Sync` batch finishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatchDoneDto {
    pub batch_index: usize,
    pub total_batches: usize,
    pub batch_processed: usize,
    pub batch_failed: usize,
    pub filed: Vec<SyncFiledPreview>,
    pub error: Option<String>,
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub completed: usize,
    pub total: usize,
    pub backend_used: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_usage: Option<SyncUsageDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cumulative_usage: Option<SyncUsageDto>,
    pub plane: PlaneResponse,
}

/// NDJSON event streamed by `POST /api/sync/stream` for real-time batch widget updates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SyncStreamEvent {
    #[serde(rename_all = "camelCase")]
    Start {
        total: usize,
        total_batches: usize,
        batch_size: usize,
        backend: String,
    },
    #[serde(rename_all = "camelCase")]
    BatchStart {
        batch_index: usize,
        total_batches: usize,
        batch_size: usize,
        items: Vec<SyncBatchPreviewItem>,
    },
    Heartbeat,
    BatchDone(Box<SyncBatchDoneDto>),
    #[serde(rename_all = "camelCase")]
    Complete {
        summary: SyncSummaryDto,
        plane: Box<PlaneResponse>,
    },
}

/// Result returned by `POST /api/processor/test`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorTestResult {
    pub ok: bool,
    pub backend_used: String,
    pub latency_ms: u64,
    pub message: String,
}

/// Compact 5-element timeline burst tuple matching `ui/app.js`:
/// `[dayIdx, startHour, endHour, promptHours[], activeMinutes]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BurstTuple(pub usize, pub f64, pub f64, pub Vec<f64>, pub i64);

/// Session record belonging to a Line of work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub harness: String,
    pub title: String,
    pub raw_title: String,
    pub steps: u64,
    pub user_input_count: u64,
    pub recency: String,
    pub running: bool,
    pub stopped_at: String,
    pub left_open: String,
    pub depends_on_id: Option<String>,
    pub hidden: bool,
    pub archived: bool,
    pub cwd: Option<String>,
    pub source_path: String,
    #[serde(default = "default_true")]
    pub source_exists: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    pub last_active_at: i64,
    pub bursts: Vec<BurstTuple>,
}

/// Semantic Line of work rendered as a parent swimlane and collapsible list group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub color: String,
    pub pinned: bool,
    pub is_system_bucket: bool,
    pub priority: u8,
    pub status: String,
    pub sessions: Vec<SessionDto>,
}

/// Multi-turn session (`user_input_count >= fewest_user_inputs`) not yet summarized or assigned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsummarizedDto {
    pub id: String,
    pub harness: String,
    pub first_prompt: String,
    pub resolved_title: Option<String>,
    pub target_line_id: Option<String>,
    pub steps: u64,
    pub user_input_count: u64,
    pub recency: String,
    pub running: bool,
    pub archived: bool,
    pub cwd: Option<String>,
    pub source_path: String,
    #[serde(default = "default_true")]
    pub source_exists: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    pub last_active_at: i64,
    pub bursts: Vec<BurstTuple>,
}

/// Short session (`user_input_count < fewest_user_inputs`) displayed under `Short sessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortItemDto {
    pub id: String,
    pub harness: String,
    pub prompt: String,
    pub recency: String,
    pub running: bool,
    pub day_idx: usize,
    pub mins: i64,
    pub steps: u64,
    pub user_input_count: u64,
    pub cwd: Option<String>,
    pub source_path: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    pub last_active_at: i64,
    pub bursts: Vec<BurstTuple>,
}

/// Aggregated short-session summary for the selected window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortSessionsDto {
    pub total_count: usize,
    pub items: Vec<ShortItemDto>,
    pub week_minutes: i64,
    #[serde(default)]
    pub week_tokens: u64,
    #[serde(default)]
    pub week_cost_usd: f64,
}

/// Complete state payload consumed by `./ui` from `GET /api/plane` and mutation endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaneResponse {
    pub days: Vec<DayMeta>,
    pub anchor_date: String,
    pub now_hour: f64,
    pub tz_label: String,
    pub archived_count: usize,
    pub settings: AppSettings,
    pub lines: Vec<LineDto>,
    pub unsummarized: Vec<UnsummarizedDto>,
    pub short_sessions: ShortSessionsDto,
    pub contributions: Vec<ContributionDayDto>,
    pub harnesses: Vec<HarnessStatusDto>,
    pub last_sync: Option<SyncSummaryDto>,
}

/// Incremental scanner telemetry returned by `planar scan`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub files_discovered: usize,
    pub files_unchanged: usize,
    pub files_full_scanned: usize,
    pub files_incremental: usize,
    pub files_errored: usize,
    pub subagents_linked: usize,
    pub elapsed_ms: u64,
}

/// L0 status summary returned by `planar status` and `planar_status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub generated_at_local: String,
    pub tz_label: String,
    pub window_days: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cwd_line_ids: Vec<String>,
    pub total_today_minutes: i64,
    pub total_window_minutes: i64,
    #[serde(default)]
    pub total_window_tokens: u64,
    #[serde(default)]
    pub total_window_cost_usd: f64,
    pub harness_counts: Vec<HarnessCount>,
    pub lines: Vec<StatusLineSummary>,
    #[serde(default)]
    pub live_sessions: Vec<LiveSessionSummary>,
    pub unsummarized_count: usize,
    pub unsummarized_today_minutes: i64,
    pub unsummarized_window_minutes: i64,
    #[serde(default)]
    pub unsummarized_window_tokens: u64,
    #[serde(default)]
    pub unsummarized_window_cost_usd: f64,
    pub top_unsummarized: Vec<StatusSessionPreview>,
    pub short_count: usize,
    pub short_today_minutes: i64,
    pub short_window_minutes: i64,
    #[serde(default)]
    pub short_window_tokens: u64,
    #[serde(default)]
    pub short_window_cost_usd: f64,
    pub dependencies: Vec<StatusDependency>,
}

/// Active session count grouped by harness name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCount {
    pub harness: String,
    pub count: usize,
}

/// Per-line summary inside `planar status` / `planar_status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusLineSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub pinned: bool,
    pub priority: u8,
    pub status: String,
    #[serde(default)]
    pub matches_cwd: bool,
    pub today_minutes: i64,
    pub window_minutes: i64,
    #[serde(default)]
    pub window_tokens: u64,
    #[serde(default)]
    pub window_cost_usd: f64,
    pub active_today_sessions: usize,
    #[serde(default)]
    pub running_sessions: usize,
    pub total_sessions: usize,
    #[serde(default)]
    pub open_followups: usize,
    #[serde(default)]
    pub last_active_recency: String,
}

/// Live sibling session (`running` or `has_open_turn`) surfaced in L0 `planar_status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSessionSummary {
    pub id: String,
    pub harness: String,
    pub line_id: Option<String>,
    pub line_name: Option<String>,
    pub cwd: Option<String>,
    pub matches_cwd: bool,
    pub has_open_turn: bool,
    pub steps: u64,
    pub recency: String,
    pub label: String,
}

/// Compact preview of an unsummarized session in `planar status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSessionPreview {
    pub id: String,
    pub harness: String,
    pub recency: String,
    pub running: bool,
    pub steps: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    pub prompt: String,
}

/// Cross-session dependency edge rendered in `planar status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDependency {
    pub from_session_id: String,
    pub to_session_id: String,
    pub kind: String,
    pub source: String,
}

/// Compact Line hint returned when a batch `L1` query includes unresolved identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineHint {
    pub id: String,
    pub name: String,
    pub priority: u8,
    pub status: String,
}

/// Directed session dependency link (`depends_on` upstream or `depended_by` downstream).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDepLink {
    pub session_id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub line_id: Option<String>,
    pub running: bool,
}

/// Ordered session digest returned inside L1 `planar line` / `planar_line`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSessionDigestDto {
    pub id: String,
    pub harness: String,
    pub title: String,
    pub stopped_at: String,
    pub left_open: String,
    pub running: bool,
    pub has_open_turn: bool,
    pub archived: bool,
    pub recency: String,
    pub last_active_at: i64,
    pub last_active_local: String,
    pub active_minutes: i64,
    pub steps: u64,
    pub user_input_count: u64,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
    pub cwd: Option<String>,
    pub depends_on: Vec<SessionDepLink>,
    pub depended_by: Vec<SessionDepLink>,
}

/// Detailed L1 Line payload returned by `planar line` / `planar_line`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineDetailDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pinned: bool,
    pub is_system_bucket: bool,
    pub priority: u8,
    pub status: String,
    pub today_minutes: i64,
    pub week_minutes: i64,
    pub total_minutes: i64,
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub total_sessions: usize,
    pub returned_sessions: usize,
    pub archived_sessions: usize,
    pub running_sessions: usize,
    pub sessions: Vec<LineSessionDigestDto>,
}

/// Batch L1 query response (`1..=8` Lines, partial-match tolerant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchLinesResponse {
    pub lines: Vec<LineDetailDto>,
    pub not_found: Vec<String>,
    pub available_lines: Vec<LineHint>,
}

/// Trajectory slice focus mode for L2 `planar session` / `planar_session`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionFocus {
    /// Full Head–Middle–Tail slice (`head + middle_users + tail`).
    #[default]
    Full,
    /// Handoff-focused slice (`digest + middle_users + tail`, omitting `head` to save tokens).
    Handoff,
}

impl SessionFocus {
    /// Parses `"full"` or `"handoff"` (case-insensitive).
    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "handoff" => Some(Self::Handoff),
            _ => None,
        }
    }

    /// Canonical lowercase label (`"full"` or `"handoff"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Handoff => "handoff",
        }
    }
}

/// Single stripped dialogue turn inside an L2 session slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlicedTurnDto {
    pub index: usize,
    pub role: String,
    pub text: String,
}

/// Stored AI digest summary attached to an L2 session slice when available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDigestSummaryDto {
    pub title: String,
    pub stopped_at: String,
    pub left_open: String,
}

/// Candidate hint returned when a short session prefix/suffix matches multiple sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousSessionHint {
    pub query: String,
    pub candidates: Vec<String>,
}

/// Bounded-batch L2 session trajectory slice returned by `planar session` / `planar_session`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSliceDto {
    pub id: String,
    pub harness: String,
    pub line_id: Option<String>,
    pub line_name: Option<String>,
    pub cwd: Option<String>,
    pub source_path: String,
    pub source_exists: bool,
    pub focus: SessionFocus,
    pub running: bool,
    pub has_open_turn: bool,
    pub archived: bool,
    pub recency: String,
    pub last_active_local: String,
    pub active_minutes: i64,
    pub steps: u64,
    pub extracted_dialogue_steps: usize,
    pub user_input_count: u64,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
    pub first_user_prompt: String,
    pub digest: Option<SessionDigestSummaryDto>,
    pub depends_on: Vec<SessionDepLink>,
    pub depended_by: Vec<SessionDepLink>,
    pub head: Vec<SlicedTurnDto>,
    pub middle_users: Vec<SlicedTurnDto>,
    pub tail: Vec<SlicedTurnDto>,
}

/// Bounded-batch L2 query response (`1..=3` sessions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSessionsResponse {
    pub focus: SessionFocus,
    pub sessions: Vec<SessionSliceDto>,
    pub not_found: Vec<String>,
    pub ambiguous: Vec<AmbiguousSessionHint>,
}

/// Query parameters for FTS5 `planar search` / `planar_search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub query: String,
    pub line: Option<String>,
    pub days: Option<usize>,
    pub limit: usize,
}

/// Single ranked hit returned by FTS5 `planar search` / `planar_search`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    pub session_id: String,
    pub harness: String,
    pub line_id: Option<String>,
    pub line_name: Option<String>,
    pub title: String,
    pub stopped_at: String,
    pub left_open: String,
    pub first_user_prompt: String,
    pub running: bool,
    pub archived: bool,
    pub recency: String,
    pub last_active_local: String,
    pub active_minutes: i64,
    pub steps: u64,
    pub total_tokens: u64,
    pub cost_usd: Option<f64>,
    pub cwd: Option<String>,
}

/// Full-text search response returned by `planar search` / `planar_search`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub line_filter: Option<String>,
    pub days: Option<usize>,
    pub hits: Vec<SearchHitDto>,
}

/// Three-state `lineId` update for `PATCH /api/sessions/{id}` (omitted vs `null` vs string).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LineIdUpdate {
    #[default]
    Unchanged,
    Clear,
    Set(String),
}

impl<'de> Deserialize<'de> for LineIdUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(id) => Self::Set(id),
            None => Self::Clear,
        })
    }
}
