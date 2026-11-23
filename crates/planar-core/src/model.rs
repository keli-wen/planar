use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `OKLab` & CVD-validated categorical line palette (`contexts/project/design.md §4.3`).
pub const PALETTE: [&str; 6] = [
    "#A43D74", // Rose Berry
    "#2E8540", // Forest Green
    "#784BA8", // Iris Violet
    "#B84A32", // Terracotta Red
    "#008778", // Slate Teal
    "#2656C4", // Cobalt Blue
];

/// Dark-surface (`#121418`) `OKLab` & CVD-validated categorical line palette.
pub const DARK_PALETTE: [&str; 6] = [
    "#D8659D", // Soft Rose
    "#3BA755", // Vivid Meadow
    "#9B72CF", // Bright Iris
    "#D9684E", // Warm Coral
    "#1CA99A", // Luminous Teal
    "#4B7BEC", // Bright Cobalt
];

/// Sentinel marker placed inside internal CLI digest prompts so scanners never re-ingest them.
pub const INTERNAL_DIGEST_SENTINEL: &str = "[PLANAR_INTERNAL_DIGEST]";

/// Reserved slug for the pinned `One-off` system bucket line.
pub const ONE_OFF_LINE_ID: &str = "one-off";

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
        Self {
            antigravity: true,
            claude: true,
            codex: true,
            dsh: true,
        }
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HarnessPathOverrides {
    pub antigravity: String,
    pub claude: String,
    pub codex: String,
    pub dsh: String,
}

const fn default_true() -> bool {
    true
}

fn default_metric_lens() -> String {
    "time".to_string()
}

/// User settings persisted in SQLite and shared with `./ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
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
    #[serde(default = "default_metric_lens")]
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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_sync: "Every day".to_string(),
            fewest_user_inputs: 3,
            show_short_on_timeline: false,
            include_archived: false,
            opening_steps: 20,
            closing_steps: 40,
            model: "Flash Lite".to_string(),
            longest_user_chars: 1200,
            longest_agent_chars: 1500,
            sessions_shown_per_line: 8,
            language: "zh".to_string(),
            theme: "system".to_string(),
            metric_lens: default_metric_lens(),
            show_contribution_graph: true,
            processor_configured: false,
            processor_mode: "cli".to_string(),
            cli_harness: String::new(),
            cli_model: String::new(),
            cli_bin_override: String::new(),
            api_provider: "gemini".to_string(),
            api_base_url: String::new(),
            api_model: "gemini-2.5-flash-lite".to_string(),
            api_key: String::new(),
            sync_batch_limit: 4,
            enabled_harnesses: EnabledHarnesses::default(),
            harness_paths: HarnessPathOverrides::default(),
        }
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

/// Outcome summary attached to `/api/sync` and `/api/regroup` responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummaryDto {
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub backend_used: String,
    pub last_error: Option<String>,
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
