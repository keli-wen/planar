//! Offline token usage accounting and API list-price USD estimation powered by `genai-prices`.
//!
//! Computes API-equivalent USD spend from `(input_uncached, cache_read, cache_write, output,
//! thinking)` token counts and the observed model identifier without making any network requests
//! on the hot path. Uses the bundled `genai-prices` snapshot by default, with an optional local
//! cache loader (`~/.planar/pricing-cache.json`) that can be refreshed asynchronously from
//! [`genai_prices::REMOTE_DATA_URL`] and automatically falls back to the bundled snapshot.
//!
//! Historical session totals (`estimate_cost_usd`) use base-tier per-1M rates so multi-turn
//! cumulative totals never falsely trigger single-request long-context tiers. Direct single-call
//! pricing (`estimate_api_cost_usd`) applies full per-request pricing when a known billing
//! provider is present, returning `None` for local/unmapped gateways (`ollama`,
//! `openai-compatible`, etc.) or missing/unknown models rather than fabricating a figure.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use genai_prices::{Calculator, PriceCalculation, PriceRequest, Usage, UsageKey};

use crate::error::{PlanarError, Result};

/// Re-exported upstream v2 provider pricing feed URL for optional background cache refreshes.
pub use genai_prices::REMOTE_DATA_URL;

/// Default filename inside `~/.planar/` for an optional cached `genai-prices` snapshot.
pub const PRICING_CACHE_FILENAME: &str = "pricing-cache.json";

static CACHE_PUBLISH_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static CACHE_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Per-million-token USD list rates probed from `genai-prices` at base tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    /// Uncached input rate (USD per 1M tokens).
    pub input_per_m: f64,
    /// Cache-hit / cached-input read rate (USD per 1M tokens).
    pub cache_read_per_m: f64,
    /// Prompt-cache creation / write rate (USD per 1M tokens).
    pub cache_write_per_m: f64,
    /// Output / completion + reasoning rate (USD per 1M tokens).
    pub output_per_m: f64,
}

impl ModelPricing {
    /// Computes USD cost for the given token breakdown using flat base rates.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn cost_usd(
        self,
        input_uncached: u64,
        cache_read: u64,
        cache_write: u64,
        output: u64,
    ) -> f64 {
        let total = (input_uncached as f64) * self.input_per_m
            + (cache_read as f64) * self.cache_read_per_m
            + (cache_write as f64) * self.cache_write_per_m
            + (output as f64) * self.output_per_m;
        total / 1_000_000.0
    }
}

#[derive(Clone)]
enum ActiveCalculator {
    Bundled,
    Cached(Arc<Calculator>),
}

static ACTIVE_CALCULATOR: LazyLock<RwLock<ActiveCalculator>> = LazyLock::new(|| {
    #[cfg(not(test))]
    if let Some(cache_path) = default_pricing_cache_path()
        && let Some(calc) = load_calculator_from_cache(&cache_path)
    {
        return RwLock::new(ActiveCalculator::Cached(Arc::new(calc)));
    }
    RwLock::new(ActiveCalculator::Bundled)
});

/// Resolves the default local pricing cache path (`$PLANAR_PRICING_CACHE_PATH` or
/// `~/.planar/pricing-cache.json`).
#[must_use]
pub fn default_pricing_cache_path() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os("PLANAR_PRICING_CACHE_PATH") {
        let trimmed = raw.to_string_lossy();
        if !trimmed.trim().is_empty() {
            return Some(PathBuf::from(trimmed.trim()));
        }
    }
    dirs::home_dir().map(|home| home.join(".planar").join(PRICING_CACHE_FILENAME))
}

/// Attempts to load a `genai-prices` [`Calculator`] from a local cached JSON snapshot without
/// touching the network. Returns `None` when the cache file is absent, unreadable, or invalid.
#[must_use]
pub fn load_calculator_from_cache(path: &Path) -> Option<Calculator> {
    let bytes = fs::read(path).ok()?;
    Calculator::from_json(&bytes).ok()
}

/// Reloads the process-wide pricing engine from `path` if it contains a valid v2 snapshot.
/// Returns `true` if the cache was activated, or `false` (leaving the existing snapshot intact).
pub fn reload_pricing_cache(path: &Path) -> bool {
    let _publish_guard = CACHE_PUBLISH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(calc) = load_calculator_from_cache(path) else {
        return false;
    };
    let mut guard = ACTIVE_CALCULATOR
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = ActiveCalculator::Cached(Arc::new(calc));
    true
}

fn unique_sibling_temp_path(cache_path: &Path) -> PathBuf {
    let seq = CACHE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let base_name = cache_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(PRICING_CACHE_FILENAME);
    let tmp_name = format!(".{base_name}.{pid}.{seq}.tmp");
    cache_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from(&tmp_name), |parent| parent.join(&tmp_name))
}

fn publish_pricing_cache_bytes(
    cache_path: &Path,
    bytes: &[u8],
    activate_global: bool,
) -> Result<()> {
    let calc = Calculator::from_json(bytes).map_err(|err| {
        PlanarError::InvalidInput(format!("invalid pricing snapshot JSON: {err}"))
    })?;

    if let Some(parent) = cache_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = unique_sibling_temp_path(cache_path);
    if let Err(err) = fs::write(&tmp_path, bytes) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }

    let _publish_guard = CACHE_PUBLISH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(err) = fs::rename(&tmp_path, cache_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }

    if activate_global {
        let mut guard = ACTIVE_CALCULATOR
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = ActiveCalculator::Cached(Arc::new(calc));
    }
    Ok(())
}

/// Downloads the latest pricing snapshot from [`REMOTE_DATA_URL`], validates it offline with
/// [`Calculator::from_json`], atomically publishes it via a unique sibling temp file, and
/// hot-swaps the active calculator off the async executor without blocking startup.
pub async fn refresh_pricing_cache(client: &reqwest::Client, cache_path: &Path) -> Result<()> {
    let resp =
        client.get(REMOTE_DATA_URL).send().await.map_err(|err| {
            PlanarError::External(format!("pricing refresh request failed: {err}"))
        })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(PlanarError::External(format!(
            "pricing refresh HTTP {status}"
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|err| PlanarError::External(format!("pricing refresh read failed: {err}")))?;

    let target = cache_path.to_path_buf();
    tokio::task::spawn_blocking(move || publish_pricing_cache_bytes(&target, &bytes, true))
        .await
        .map_err(|err| PlanarError::TaskJoin(err.to_string()))?
}

fn with_active_calculator<T>(f: impl FnOnce(&Calculator) -> T) -> T {
    let snapshot = ACTIVE_CALCULATOR
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    match snapshot {
        ActiveCalculator::Bundled => f(Calculator::bundled()),
        ActiveCalculator::Cached(calc) => f(&calc),
    }
}

/// Resolves offline base list-price rates (USD per 1M tokens) for a model identifier, if recognized.
#[must_use]
pub fn lookup_model_pricing(raw_model: &str) -> Option<ModelPricing> {
    with_active_calculator(|calc| lookup_model_pricing_with_calculator(calc, raw_model))
}

fn lookup_model_pricing_with_calculator(
    calc: &Calculator,
    raw_model: &str,
) -> Option<ModelPricing> {
    // Probe with 1,000 tokens (below any >200K/>272K context tier) and scale to 1M tokens.
    const PROBE_TOKENS: f64 = 1_000.0;
    const SCALE_TO_M: f64 = 1_000.0;

    let input_usage = Usage::from([(UsageKey::INPUT_TOKENS, PROBE_TOKENS)]);
    let input_calc = calculate_normalized(calc, None, raw_model, &input_usage)?;
    let input_per_m = input_calc.input_price * SCALE_TO_M;

    let provider_id = input_calc.provider_id.as_str();
    let model_id = input_calc.model_id.as_str();

    let cache_read_usage = Usage::from([
        (UsageKey::INPUT_TOKENS, PROBE_TOKENS),
        (UsageKey::CACHE_READ_TOKENS, PROBE_TOKENS),
    ]);
    let cache_read_per_m = calc
        .calculate(&PriceRequest::new(model_id, &cache_read_usage).with_provider_id(provider_id))
        .map_or(input_per_m, |p| p.input_price * SCALE_TO_M);

    let cache_write_usage = Usage::from([
        (UsageKey::INPUT_TOKENS, PROBE_TOKENS),
        (UsageKey::CACHE_WRITE_TOKENS, PROBE_TOKENS),
    ]);
    let cache_write_per_m = calc
        .calculate(&PriceRequest::new(model_id, &cache_write_usage).with_provider_id(provider_id))
        .map_or(input_per_m, |p| p.input_price * SCALE_TO_M);

    let output_usage = Usage::from([(UsageKey::OUTPUT_TOKENS, PROBE_TOKENS)]);
    let output_per_m = calc
        .calculate(&PriceRequest::new(model_id, &output_usage).with_provider_id(provider_id))
        .map_or(0.0, |p| p.output_price * SCALE_TO_M);

    Some(ModelPricing {
        input_per_m,
        cache_read_per_m,
        cache_write_per_m,
        output_per_m,
    })
}

/// Estimates API-equivalent USD cost for historical aggregated session totals when `model` has
/// known pricing.
///
/// Intentionally uses base-tier per-1M rates (excluding single-request long-context tiers)
/// because harness session rows aggregate tokens across many turns without per-request sizes.
#[must_use]
pub fn estimate_cost_usd(
    model: Option<&str>,
    input_uncached: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
) -> Option<f64> {
    if input_uncached == 0 && cache_read == 0 && cache_write == 0 && output == 0 {
        return None;
    }
    let pricing = lookup_model_pricing(model?)?;
    let cost = pricing.cost_usd(input_uncached, cache_read, cache_write, output);
    (cost.is_finite() && cost > 0.0).then_some(cost)
}

/// Estimates USD cost for one direct API request (separating `thinking` / reasoning tokens from
/// visible `output` tokens and applying single-request context-size tiers).
///
/// When `provider` is `Some(...)`, it MUST map to a known `genai-prices` billing provider
/// (e.g. `gemini`, `anthropic`, `openai`, `deepseek`, `openrouter`, `xai`, `groq`, `mistral`,
/// `moonshot`). Local or generic gateways (`ollama`, `openai-compatible`, custom endpoints)
/// return `None` rather than attributing another vendor's USD list price.
#[must_use]
pub fn estimate_api_cost_usd(
    provider: Option<&str>,
    model: Option<&str>,
    input_uncached: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
    thinking: u64,
) -> Option<f64> {
    let known_provider = match provider {
        Some(raw) => Some(canonical_billing_provider(raw)?),
        None => None,
    };
    let raw_model = model?;
    with_active_calculator(|calc| {
        estimate_api_cost_with_calculator(
            calc,
            known_provider,
            raw_model,
            input_uncached,
            cache_read,
            cache_write,
            output,
            thinking,
        )
    })
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn estimate_api_cost_with_calculator(
    calc: &Calculator,
    known_provider: Option<&str>,
    raw_model: &str,
    input_uncached: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
    thinking: u64,
) -> Option<f64> {
    let total_input = input_uncached
        .saturating_add(cache_read)
        .saturating_add(cache_write);
    let total_output = output.saturating_add(thinking);
    if total_input == 0 && total_output == 0 {
        return None;
    }

    // In `genai-prices`, `input_tokens` is the ancestor (total input including cached) and
    // `output_tokens` is the ancestor (total output including reasoning). The engine subtracts
    // priced descendants (`cache_read_tokens`, `cache_write_tokens`, `output_reasoning_tokens`)
    // automatically to compute leaf token counts.
    let mut usage = Usage::new();
    if total_input > 0 {
        usage.insert(UsageKey::INPUT_TOKENS, total_input as f64);
    }
    if cache_read > 0 {
        usage.insert(UsageKey::CACHE_READ_TOKENS, cache_read as f64);
    }
    if cache_write > 0 {
        usage.insert(UsageKey::CACHE_WRITE_TOKENS, cache_write as f64);
    }
    if total_output > 0 {
        usage.insert(UsageKey::OUTPUT_TOKENS, total_output as f64);
    }
    if thinking > 0 {
        usage.insert(UsageKey::OUTPUT_REASONING_TOKENS, thinking as f64);
    }

    let calc_res = calculate_normalized(calc, known_provider, raw_model, &usage)?;
    (calc_res.total_price.is_finite() && calc_res.total_price > 0.0).then_some(calc_res.total_price)
}

fn calculate_normalized(
    calc: &Calculator,
    explicit_provider: Option<&str>,
    raw_model: &str,
    usage: &Usage,
) -> Option<PriceCalculation> {
    let trimmed = raw_model.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if is_ignored_model_sentinel(&lower) {
        return None;
    }

    let (prefix_provider, stripped) = strip_provider_prefix(&lower);
    if stripped.is_empty() || is_ignored_model_sentinel(stripped) {
        return None;
    }

    let preferred_provider = explicit_provider.or(prefix_provider);

    // 1. Direct lookup with preferred provider hint on raw and prefix-stripped identifiers.
    for candidate in [
        Some(lower.as_str()),
        (stripped != lower).then_some(stripped),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(res) = try_calculate(calc, candidate, preferred_provider, usage) {
            return Some(res);
        }
    }

    // 2. Fallback: strip harness effort suffixes (`-low`, `-medium`, `-high`, `-xhigh`) on miss.
    let without_effort =
        strip_effort_suffix(stripped).filter(|candidate| !is_ignored_model_sentinel(candidate));
    if let Some(candidate) = without_effort
        && let Some(res) = try_calculate(calc, candidate, preferred_provider, usage)
    {
        return Some(res);
    }

    // 3. When no explicit billing provider was asserted (e.g. historical harness transcript lookup),
    // or when using a multi-vendor router (`openrouter`), retry provider auto-detection on miss.
    let allow_cross_provider_fallback =
        explicit_provider.is_none() || explicit_provider == Some("openrouter");
    if preferred_provider.is_some() && allow_cross_provider_fallback {
        if let Some(res) = try_calculate(calc, stripped, None, usage) {
            return Some(res);
        }
        if let Some(candidate) = without_effort
            && let Some(res) = try_calculate(calc, candidate, None, usage)
        {
            return Some(res);
        }
    }

    None
}

fn try_calculate(
    calc: &Calculator,
    model_id: &str,
    provider_id: Option<&str>,
    usage: &Usage,
) -> Option<PriceCalculation> {
    let mut req = PriceRequest::new(model_id, usage);
    if let Some(pid) = provider_id {
        req = req.with_provider_id(pid);
    }
    calc.calculate(&req).ok()
}

fn is_ignored_model_sentinel(lower: &str) -> bool {
    matches!(lower, "" | "unknown" | "synthetic" | "<synthetic>" | "auto")
}

/// Maps a Planar API provider ID to its canonical `genai-prices` billing provider ID.
/// Returns `None` for local or generic proxy providers (`ollama`, `openai-compatible`, etc.)
/// so Planar never attributes third-party USD prices to an unbilled or custom endpoint.
pub(crate) fn canonical_billing_provider(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "gemini" | "google" | "vertex" => Some("google"),
        "anthropic" => Some("anthropic"),
        "openai" => Some("openai"),
        "deepseek" | "deepseek-ai" => Some("deepseek"),
        "xai" | "x-ai" => Some("x-ai"),
        "moonshot" | "moonshotai" => Some("moonshotai"),
        "groq" => Some("groq"),
        "mistral" => Some("mistral"),
        "openrouter" => Some("openrouter"),
        _ => None,
    }
}

fn strip_provider_prefix(lower: &str) -> (Option<&'static str>, &str) {
    const PREFIXES: [(&str, &str); 10] = [
        ("anthropic/", "anthropic"),
        ("openai/", "openai"),
        ("google/", "google"),
        ("models/", "google"),
        ("deepseek/", "deepseek"),
        ("deepseek-ai/", "deepseek"),
        ("x-ai/", "x-ai"),
        ("xai/", "x-ai"),
        ("moonshotai/", "moonshotai"),
        ("moonshot/", "moonshotai"),
    ];
    for (prefix, provider) in PREFIXES {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return (Some(provider), rest);
        }
    }
    (None, lower)
}

fn strip_effort_suffix(lower: &str) -> Option<&str> {
    for suffix in ["-xhigh", "-high", "-medium", "-low"] {
        if let Some(base) = lower.strip_suffix(suffix)
            && !base.is_empty()
        {
            return Some(base);
        }
    }
    None
}

/// Formats a compact human-readable token count (`0`, `840`, `42.5K`, `14.8M`, `1.24B`).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn format_tokens_compact(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        let k = (tokens as f64) / 1_000.0;
        return if k >= 100.0 {
            format!("{k:.0}K")
        } else {
            trim_zero_tenth(k, "K")
        };
    }
    if tokens < 1_000_000_000 {
        let m = (tokens as f64) / 1_000_000.0;
        return if m >= 100.0 {
            format!("{m:.0}M")
        } else {
            trim_zero_tenth(m, "M")
        };
    }
    let b = (tokens as f64) / 1_000_000_000.0;
    let formatted = format!("{b:.2}");
    let clean = formatted.trim_end_matches('0').trim_end_matches('.');
    format!("{clean}B")
}

/// Formats a USD cost cleanly for CLI/UI readouts (`<$0.01`, `$0.03`, `$1.25`, `$142`).
#[must_use]
pub fn format_usd_compact(usd: f64) -> String {
    if !usd.is_finite() || usd <= 0.0 {
        return "$0.00".to_string();
    }
    if usd < 0.005 {
        return "<$0.01".to_string();
    }
    if usd >= 100.0 {
        return format!("${usd:.0}");
    }
    format!("${usd:.2}")
}

fn trim_zero_tenth(val: f64, suffix: &str) -> String {
    let s = format!("{val:.1}");
    let clean = s.strip_suffix(".0").unwrap_or(&s);
    format!("{clean}{suffix}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn gemini_4_and_35_models_price_accurately_including_effort_and_prefix_normalization() {
        // gemini-4-hydrogen: $1.50/M input, $0.075/M cache read, $7.50/M output
        let hydrogen =
            estimate_cost_usd(Some("gemini-4-hydrogen"), 100_000, 200_000, 0, 20_000).unwrap();
        // 0.1 * 1.50 ($0.15) + 0.2 * 0.075 ($0.015) + 0.02 * 7.50 ($0.15) = $0.315
        assert!(
            (hydrogen - 0.315).abs() < 1e-6,
            "unexpected gemini-4-hydrogen cost: {hydrogen}"
        );

        // Prefixed `models/gemini-4-hydrogen` resolves identically.
        let prefixed = estimate_cost_usd(
            Some("models/gemini-4-hydrogen"),
            100_000,
            200_000,
            0,
            20_000,
        )
        .unwrap();
        assert!((prefixed - hydrogen).abs() < 1e-9);

        // Effort suffix `-high` stripped on miss -> `gemini-4-neon` ($3/M in, $0.15/M cached, $15/M out)
        let neon_high =
            estimate_cost_usd(Some("gemini-4-neon-high"), 100_000, 0, 0, 10_000).unwrap();
        // 0.1 * 3.00 ($0.30) + 0.01 * 15.00 ($0.15) = $0.45
        assert!(
            (neon_high - 0.45).abs() < 1e-6,
            "unexpected gemini-4-neon-high cost: {neon_high}"
        );

        // gemini-3.5-flash-lite: $0.30/M input, $0.03/M cache read, $2.50/M output
        let flash_lite = estimate_cost_usd(
            Some("google/gemini-3.5-flash-lite"),
            1_000_000,
            0,
            0,
            100_000,
        )
        .unwrap();
        // 1.0 * 0.30 ($0.30) + 0.1 * 2.50 ($0.25) = $0.55
        assert!(
            (flash_lite - 0.55).abs() < 1e-6,
            "unexpected gemini-3.5-flash-lite cost: {flash_lite}"
        );

        // Thinking tokens are billed as output tokens alongside visible candidates.
        let with_thinking = estimate_api_cost_usd(
            Some("gemini"),
            Some("gemini-4-hydrogen"),
            100_000,
            0,
            0,
            10_000,
            10_000,
        )
        .unwrap();
        // 0.1 * 1.50 ($0.15) + 0.02 * 7.50 ($0.15) = $0.30
        assert!(
            (with_thinking - 0.30).abs() < 1e-6,
            "unexpected thinking cost: {with_thinking}"
        );
    }

    #[test]
    fn historical_session_totals_stay_on_base_rates_without_request_size_tiers() {
        // gemini-2.5-pro has a >200K single-request tier ($2.50/$15 vs base $1.25/$10).
        // Aggregated historical session totals (e.g. 300K input + 20K output across many turns)
        // must NOT trigger the >200K single-request tier: 0.3 * 1.25 + 0.02 * 10 = $0.575.
        let session_cost =
            estimate_cost_usd(Some("gemini-2.5-pro"), 300_000, 0, 0, 20_000).unwrap();
        assert!(
            (session_cost - 0.575).abs() < 1e-6,
            "expected base-tier historical session cost $0.575, got {session_cost}"
        );

        // By contrast, a single direct API request of 300K input + 20K output DOES apply the >200K tier.
        let single_call_cost = estimate_api_cost_usd(
            Some("gemini"),
            Some("gemini-2.5-pro"),
            300_000,
            0,
            0,
            20_000,
            0,
        )
        .unwrap();
        assert!(
            single_call_cost > session_cost + 0.2,
            "expected tiered single-request cost > $0.775, got {single_call_cost}"
        );
    }

    #[test]
    fn claude_sonnet_applies_cache_read_and_write_discounts() {
        // 100k uncached ($0.30) + 1M cache read ($0.30) + 100k cache write ($0.375) + 20k output ($0.30) = $1.275
        let cost = estimate_cost_usd(
            Some("claude-sonnet-4-20250514"),
            100_000,
            1_000_000,
            100_000,
            20_000,
        )
        .unwrap();
        assert!(
            (cost - 1.275).abs() < 1e-6,
            "unexpected claude cost: {cost}"
        );
    }

    #[test]
    fn codex_gpt5_and_deepseek_models_price_accurately() {
        let cost = estimate_cost_usd(Some("gpt-5-codex"), 200_000, 2_000_000, 0, 50_000).unwrap();
        // 0.2M * 1.25 ($0.25) + 2.0M * 0.125 ($0.25) + 0.05M * 10.0 ($0.50) = $1.00
        assert!(
            (cost - 1.00).abs() < 1e-6,
            "unexpected gpt-5-codex cost: {cost}"
        );

        // Effort suffix on gpt-5-mini (`-xhigh`)
        let mini = estimate_cost_usd(Some("gpt-5-mini-xhigh"), 1_000_000, 0, 0, 100_000).unwrap();
        // 1.0 * 0.25 ($0.25) + 0.1 * 2.00 ($0.20) = $0.45
        assert!(
            (mini - 0.45).abs() < 1e-6,
            "unexpected gpt-5-mini-xhigh cost: {mini}"
        );

        // deepseek-chat has published UTC standard ($0.27/$1.10) & off-peak ($0.135/$0.55) rates.
        let ds = estimate_cost_usd(Some("deepseek-chat"), 1_000_000, 0, 0, 100_000).unwrap();
        let standard = 0.27 + 0.11;
        let off_peak = 0.135 + 0.055;
        assert!(
            (ds - standard).abs() < 1e-6 || (ds - off_peak).abs() < 1e-6,
            "unexpected deepseek-chat cost: {ds}"
        );
    }

    #[test]
    fn unknown_missing_local_or_unattributed_gateway_providers_return_none() {
        assert_eq!(estimate_cost_usd(None, 500_000, 1_000_000, 0, 10_000), None);
        assert_eq!(
            estimate_cost_usd(Some("qwen2.5-coder:32b-local"), 50_000, 0, 0, 2_000),
            None
        );
        assert_eq!(
            estimate_cost_usd(Some("unknown"), 50_000, 0, 0, 2_000),
            None
        );
        assert_eq!(estimate_cost_usd(Some("gpt-5"), 0, 0, 0, 0), None);
        // Ollama and generic `openai-compatible` / custom gateways must NOT inherit OpenAI USD rates.
        assert_eq!(
            estimate_api_cost_usd(Some("ollama"), Some("llama-3.3-70b"), 10_000, 0, 0, 500, 0),
            None
        );
        assert_eq!(
            estimate_api_cost_usd(
                Some("openai-compatible"),
                Some("gpt-4o-mini"),
                100_000,
                0,
                0,
                10_000,
                0
            ),
            None
        );
        assert_eq!(
            estimate_api_cost_usd(
                Some("siliconflow"),
                Some("gpt-4o-mini"),
                100_000,
                0,
                0,
                10_000,
                0
            ),
            None
        );
        // Known provider (`openai`) with `gpt-4o-mini` still prices accurately.
        assert!(
            estimate_api_cost_usd(
                Some("openai"),
                Some("gpt-4o-mini"),
                100_000,
                0,
                0,
                10_000,
                0
            )
            .is_some()
        );
        // Known provider (`openrouter`) with vendor-prefixed model still resolves.
        assert!(
            estimate_api_cost_usd(
                Some("openrouter"),
                Some("google/gemini-2.5-flash-lite"),
                100_000,
                0,
                0,
                10_000,
                0
            )
            .is_some()
        );
    }

    #[test]
    fn lookup_model_pricing_and_cache_loader_and_concurrent_publish_work_offline() {
        let rates = lookup_model_pricing("gemini-4-hydrogen").unwrap();
        assert!((rates.input_per_m - 1.50).abs() < 1e-6);
        assert!((rates.cache_read_per_m - 0.075).abs() < 1e-6);
        assert!((rates.output_per_m - 7.50).abs() < 1e-6);

        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("missing.json");
        assert!(load_calculator_from_cache(&missing).is_none());

        let corrupt = dir.path().join("corrupt.json");
        fs::write(&corrupt, b"{not-valid-json").unwrap();
        assert!(load_calculator_from_cache(&corrupt).is_none());

        // Overlapping cache publications use unique temp files, serialize rename, and leave no stray .tmp files.
        let target = dir.path().join(PRICING_CACHE_FILENAME);
        let invalid_res = publish_pricing_cache_bytes(&target, b"{invalid", false);
        assert!(invalid_res.is_err());
        assert!(!target.exists());

        let min_v2_json = br"[]";
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let p = target.clone();
                std::thread::spawn(move || publish_pricing_cache_bytes(&p, min_v2_json, false))
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
        assert!(target.exists());
        assert!(load_calculator_from_cache(&target).is_some());
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
    }

    #[test]
    fn compact_formatters_round_cleanly() {
        assert_eq!(format_tokens_compact(0), "0");
        assert_eq!(format_tokens_compact(942), "942");
        assert_eq!(format_tokens_compact(14_000), "14K");
        assert_eq!(format_tokens_compact(42_560), "42.6K");
        assert_eq!(format_tokens_compact(245_100), "245K");
        assert_eq!(format_tokens_compact(1_840_000), "1.8M");
        assert_eq!(format_tokens_compact(1_250_000_000), "1.25B");

        assert_eq!(format_usd_compact(0.0), "$0.00");
        assert_eq!(format_usd_compact(0.0012), "<$0.01");
        assert_eq!(format_usd_compact(0.034), "$0.03");
        assert_eq!(format_usd_compact(124.7), "$125");
    }
}
