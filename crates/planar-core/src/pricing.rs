//! Offline token usage accounting and API list-price USD estimation.
//!
//! Computes approximate API-equivalent USD spend from `(input_uncached, cache_read,
//! cache_write, output)` token counts and the observed model identifier without making
//! any network requests. When a harness transcript omits model metadata (for example,
//! Antigravity `transcript.jsonl`), [`estimate_cost_usd`] returns `None` rather than
//! fabricating a figure.

/// Per-million-token USD list rates for a model family.
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
    const fn new(
        input_per_m: f64,
        cache_read_per_m: f64,
        cache_write_per_m: f64,
        output_per_m: f64,
    ) -> Self {
        Self {
            input_per_m,
            cache_read_per_m,
            cache_write_per_m,
            output_per_m,
        }
    }

    /// Computes USD cost for the given token breakdown.
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

/// Resolves offline list-price rates for a model identifier, if recognized.
#[must_use]
pub fn lookup_model_pricing(raw_model: &str) -> Option<ModelPricing> {
    let trimmed = raw_model.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let m = strip_provider_prefix(&lower);
    if m.is_empty() || m == "unknown" || m == "synthetic" || m == "<synthetic>" || m == "auto" {
        return None;
    }

    // Anthropic Claude families
    if m.starts_with("claude-") {
        if m.contains("opus-4-5")
            || m.contains("opus-4-6")
            || m.contains("4-5-opus")
            || m.contains("4-6-opus")
            || m.contains("opus-4.5")
            || m.contains("opus-4.6")
        {
            return Some(ModelPricing::new(5.00, 0.50, 6.25, 25.00));
        }
        if m.contains("opus") {
            return Some(ModelPricing::new(15.00, 1.50, 18.75, 75.00));
        }
        if m.contains("haiku-4") || m.contains("4-5-haiku") || m.contains("haiku-4.5") {
            return Some(ModelPricing::new(1.00, 0.10, 1.25, 5.00));
        }
        if m.contains("3-5-haiku") || m.contains("3.5-haiku") {
            return Some(ModelPricing::new(0.80, 0.08, 1.00, 4.00));
        }
        if m.contains("haiku") {
            return Some(ModelPricing::new(0.25, 0.03, 0.30, 1.25));
        }
        // Sonnet 3.5 / 3.7 / 4 / 4.5 / 4.6 & default Claude fallback
        return Some(ModelPricing::new(3.00, 0.30, 3.75, 15.00));
    }

    // OpenAI GPT / Codex / o-series families
    if m.starts_with("codex-mini") {
        return Some(ModelPricing::new(1.50, 0.375, 1.50, 6.00));
    }
    if m.starts_with("gpt-5-nano") {
        return Some(ModelPricing::new(0.05, 0.005, 0.05, 0.40));
    }
    if m.starts_with("gpt-4.1-nano") {
        return Some(ModelPricing::new(0.10, 0.025, 0.10, 0.40));
    }
    if m.starts_with("gpt-5-mini") {
        return Some(ModelPricing::new(0.25, 0.025, 0.25, 2.00));
    }
    if m.starts_with("gpt-4.1-mini") {
        return Some(ModelPricing::new(0.40, 0.10, 0.40, 1.60));
    }
    if m.starts_with("gpt-4o-mini") {
        return Some(ModelPricing::new(0.15, 0.075, 0.15, 0.60));
    }
    if m.starts_with("gpt-4.1") {
        return Some(ModelPricing::new(2.00, 0.50, 2.00, 8.00));
    }
    if m.starts_with("gpt-4o") || m.starts_with("chatgpt-4o") {
        return Some(ModelPricing::new(2.50, 1.25, 2.50, 10.00));
    }
    if m.starts_with("gpt-5-pro") {
        return Some(ModelPricing::new(15.00, 1.50, 15.00, 120.00));
    }
    if m.starts_with("gpt-5") || m.starts_with("codex") {
        return Some(ModelPricing::new(1.25, 0.125, 1.25, 10.00));
    }
    if m.starts_with("o3-mini") || m.starts_with("o4-mini") || m.starts_with("o1-mini") {
        return Some(ModelPricing::new(1.10, 0.275, 1.10, 4.40));
    }
    if m.starts_with("o3-pro") || m.starts_with("o1") {
        return Some(ModelPricing::new(15.00, 7.50, 15.00, 60.00));
    }
    if m.starts_with("o3") || m.starts_with("o4") {
        return Some(ModelPricing::new(2.00, 0.50, 2.00, 8.00));
    }

    // Google Gemini families
    if m.starts_with("gemini-") {
        if m.contains("flash-lite") {
            return Some(ModelPricing::new(0.10, 0.025, 0.10, 0.40));
        }
        if m.contains("flash") {
            return Some(ModelPricing::new(0.30, 0.075, 0.30, 2.50));
        }
        return Some(ModelPricing::new(1.25, 0.3125, 1.25, 10.00));
    }

    // DeepSeek (`dsh`) families
    if m.starts_with("deepseek-") {
        if m.contains("reasoner") || m.contains("r1") {
            return Some(ModelPricing::new(0.55, 0.14, 0.55, 2.19));
        }
        return Some(ModelPricing::new(0.27, 0.07, 0.27, 1.10));
    }

    // xAI Grok & Moonshot Kimi families
    if m.starts_with("grok-") {
        if m.contains("mini") || m.contains("fast") {
            return Some(ModelPricing::new(0.30, 0.075, 0.30, 0.50));
        }
        return Some(ModelPricing::new(3.00, 0.75, 3.00, 15.00));
    }
    if m.starts_with("kimi-") || m.starts_with("moonshot-") {
        return Some(ModelPricing::new(0.60, 0.15, 0.60, 2.50));
    }

    None
}

/// Estimates API-equivalent USD cost for a token breakdown when `model` has known pricing.
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
    (cost > 0.0).then_some(cost)
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

fn trim_zero_tenth(val: f64, suffix: &str) -> String {
    let s = format!("{val:.1}");
    let clean = s.strip_suffix(".0").unwrap_or(&s);
    format!("{clean}{suffix}")
}

fn strip_provider_prefix(lower: &str) -> &str {
    for prefix in [
        "anthropic/",
        "openai/",
        "google/",
        "models/",
        "deepseek/",
        "deepseek-ai/",
        "x-ai/",
        "moonshotai/",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest;
        }
    }
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((cost - 1.275).abs() < 1e-6);
    }

    #[test]
    fn codex_and_gpt5_aliases_apply_90_percent_cached_discount() {
        let cost = estimate_cost_usd(Some("gpt-5-codex"), 200_000, 2_000_000, 0, 50_000).unwrap();
        // 0.2M * 1.25 ($0.25) + 2.0M * 0.125 ($0.25) + 0.05M * 10.0 ($0.50) = $1.00
        assert!((cost - 1.00).abs() < 1e-6);
    }

    #[test]
    fn unknown_or_missing_model_returns_none() {
        assert_eq!(estimate_cost_usd(None, 500_000, 1_000_000, 0, 10_000), None);
        assert_eq!(
            estimate_cost_usd(Some("qwen2.5-coder:32b-local"), 50_000, 0, 0, 2_000),
            None
        );
        assert_eq!(estimate_cost_usd(Some("gpt-5"), 0, 0, 0, 0), None);
    }

    #[test]
    fn compact_token_formatter_rounds_cleanly() {
        assert_eq!(format_tokens_compact(0), "0");
        assert_eq!(format_tokens_compact(942), "942");
        assert_eq!(format_tokens_compact(14_000), "14K");
        assert_eq!(format_tokens_compact(42_560), "42.6K");
        assert_eq!(format_tokens_compact(245_100), "245K");
        assert_eq!(format_tokens_compact(1_840_000), "1.8M");
        assert_eq!(format_tokens_compact(1_250_000_000), "1.25B");
    }
}
