use chrono::{DateTime, Duration, FixedOffset, LocalResult, NaiveDate, TimeZone};

use crate::model::BurstTuple;

/// Maximum idle gap between consecutive events merged into one active burst (`12 min`).
pub const IDLE_GAP_CAP_MS: i64 = 12 * 60 * 1000;

/// Minimum visual and accounting floor applied when projecting stored bursts onto days (`1 min`).
pub const MIN_BURST_MS: i64 = 60 * 1000;

/// Raw inclusive activity span `[start_ms, end_ms]` in Unix milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSpan {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl RawSpan {
    /// Creates a point span at `ts_ms`.
    pub const fn point(ts_ms: i64) -> Self {
        Self {
            start_ms: ts_ms,
            end_ms: ts_ms,
        }
    }

    /// Creates a span `[start_ms, end_ms]`, clamping `end_ms >= start_ms`.
    pub fn clamped(start_ms: i64, end_ms: i64) -> Self {
        Self {
            start_ms,
            end_ms: end_ms.max(start_ms),
        }
    }
}

/// Merges point events and explicit tool execution spans into sorted, disjoint
/// burst intervals, clamping any human idle gap strictly greater than `idle_gap_cap_ms`.
///
/// Stored intervals are kept unpadded so byte-cursor incremental appends remain associative.
pub fn merge_activity_spans(spans: &mut [RawSpan], idle_gap_cap_ms: i64) -> Vec<RawSpan> {
    let mut valid: Vec<RawSpan> = spans
        .iter()
        .copied()
        .filter(|s| s.start_ms > 0)
        .map(|s| RawSpan::clamped(s.start_ms, s.end_ms))
        .collect();

    if valid.is_empty() {
        return Vec::new();
    }

    valid.sort_unstable_by(|a, b| a.start_ms.cmp(&b.start_ms).then(a.end_ms.cmp(&b.end_ms)));

    let mut merged = Vec::with_capacity(valid.len());
    let mut cur = valid[0];

    for next in valid.into_iter().skip(1) {
        if next.start_ms.saturating_sub(cur.end_ms) <= idle_gap_cap_ms {
            cur.end_ms = cur.end_ms.max(next.end_ms);
        } else {
            merged.push(cur);
            cur = next;
        }
    }
    merged.push(cur);
    merged
}

/// Sums active duration in rounded minutes for intervals overlapping `[window_start_ms, window_end_ms)`.
pub fn sum_interval_minutes_in_range(
    intervals: &[RawSpan],
    window_start_ms: i64,
    window_end_ms: i64,
) -> i64 {
    let mut total_mins = 0_i64;
    for iv in intervals {
        let eff_end = iv.end_ms.max(iv.start_ms.saturating_add(MIN_BURST_MS));
        let clipped_start = iv.start_ms.max(window_start_ms);
        let clipped_end = eff_end.min(window_end_ms);
        if clipped_end > clipped_start {
            let mins = ((clipped_end - clipped_start) as f64 / 60_000.0)
                .round()
                .max(1.0) as i64;
            total_mins += mins;
        }
    }
    total_mins
}

/// Projects stored intervals and prompt timestamps into local-midnight-split `BurstTuple`s
/// for a `num_days` window starting at `window_start_date`.
pub fn project_bursts_into_days(
    intervals: &[RawSpan],
    prompt_timestamps_ms: &[i64],
    window_start_date: NaiveDate,
    num_days: usize,
    offset: FixedOffset,
) -> Vec<BurstTuple> {
    if intervals.is_empty() || num_days == 0 {
        return Vec::new();
    }

    let day_bounds: Vec<(i64, i64)> = (0..num_days)
        .filter_map(|idx| {
            let days = i64::try_from(idx).ok()?;
            let day = window_start_date.checked_add_signed(Duration::days(days))?;
            let next_day = day.checked_add_signed(Duration::days(1))?;
            let start_ms = local_midnight_ms(day, offset)?;
            let end_ms = local_midnight_ms(next_day, offset)?;
            Some((start_ms, end_ms))
        })
        .collect();

    if day_bounds.len() != num_days {
        return Vec::new();
    }

    let mut sorted_prompts: Vec<i64> = prompt_timestamps_ms.to_vec();
    sorted_prompts.sort_unstable();
    sorted_prompts.dedup();

    let mut out = Vec::new();
    for iv in intervals {
        let eff_end = iv.end_ms.max(iv.start_ms.saturating_add(MIN_BURST_MS));
        for (day_idx, &(day_start_ms, day_end_ms)) in day_bounds.iter().enumerate() {
            if eff_end <= day_start_ms || iv.start_ms >= day_end_ms {
                continue;
            }
            let seg_start = iv.start_ms.max(day_start_ms);
            let seg_end = eff_end.min(day_end_ms);
            if seg_end <= seg_start {
                continue;
            }

            let start_hour = round4((seg_start - day_start_ms) as f64 / 3_600_000.0);
            let end_hour = round4((seg_end - day_start_ms) as f64 / 3_600_000.0);
            let active_minutes = ((seg_end - seg_start) as f64 / 60_000.0).round().max(1.0) as i64;

            // Prompts within this day segment (allowing exact segment boundary).
            let prompt_hours: Vec<f64> = sorted_prompts
                .iter()
                .copied()
                .filter(|&p| p >= day_start_ms && p < day_end_ms && p >= seg_start && p <= seg_end)
                .map(|p| round4((p - day_start_ms) as f64 / 3_600_000.0))
                .collect();

            out.push(BurstTuple(
                day_idx,
                start_hour,
                end_hour,
                prompt_hours,
                active_minutes,
            ));
        }
    }

    out
}

/// Computes Unix milliseconds for `00:00:00` on `date` in `offset`.
pub fn local_midnight_ms(date: NaiveDate, offset: FixedOffset) -> Option<i64> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    match offset.from_local_datetime(&naive) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => Some(dt.timestamp_millis()),
        LocalResult::None => None,
    }
}

/// Formats a `FixedOffset` timezone as `UTC+8`, `UTC-5`, or `UTC+5:30`.
pub fn format_tz_label(offset: FixedOffset) -> String {
    let secs = offset.local_minus_utc();
    if secs == 0 {
        return "UTC".to_string();
    }
    let sign = if secs > 0 { '+' } else { '-' };
    let abs_secs = secs.abs();
    let hours = abs_secs / 3600;
    let mins = (abs_secs % 3600) / 60;
    if mins == 0 {
        format!("UTC{sign}{hours}")
    } else {
        format!("UTC{sign}{hours}:{mins:02}")
    }
}

/// Computes fractional local hour `0.0..24.0` from `dt`.
pub fn fractional_hour(dt: DateTime<FixedOffset>) -> f64 {
    let date = dt.date_naive();
    let Some(mid_ms) = local_midnight_ms(date, *dt.offset()) else {
        return 0.0;
    };
    round4(((dt.timestamp_millis() - mid_ms) as f64 / 3_600_000.0).clamp(0.0, 24.0))
}

fn round4(val: f64) -> f64 {
    (val * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn merge_activity_spans_merges_within_cap_and_splits_across_idle_gap() {
        let base = 1_700_000_000_000_i64;
        let min = 60_000_i64;
        let mut spans = vec![
            RawSpan::point(base + 30 * min),
            RawSpan::point(base),
            RawSpan::point(base + 5 * min),
            // 20-minute tool execution span starting 10 min after last event (<= 12 min gap)
            RawSpan::clamped(base + 15 * min, base + 35 * min),
            // Gap of 15 minutes (> 12 min) -> must start a new burst
            RawSpan::point(base + 50 * min),
        ];

        let merged = merge_activity_spans(&mut spans, IDLE_GAP_CAP_MS);
        assert_eq!(
            merged,
            vec![
                RawSpan::clamped(base, base + 35 * min),
                RawSpan::point(base + 50 * min),
            ]
        );
    }

    #[test]
    fn incremental_merge_matches_full_scan_merge() {
        let base = 1_700_000_000_000_i64;
        let min = 60_000_i64;
        let mut batch1 = vec![
            RawSpan::point(base),
            RawSpan::point(base + 4 * min),
            RawSpan::point(base + 25 * min),
        ];
        let mut merged1 = merge_activity_spans(&mut batch1, IDLE_GAP_CAP_MS);
        // Append second batch bridging 25m -> 35m
        merged1.push(RawSpan::clamped(base + 35 * min, base + 42 * min));
        let incremental = merge_activity_spans(&mut merged1, IDLE_GAP_CAP_MS);

        let mut all = vec![
            RawSpan::point(base),
            RawSpan::point(base + 4 * min),
            RawSpan::point(base + 25 * min),
            RawSpan::clamped(base + 35 * min, base + 42 * min),
        ];
        let full = merge_activity_spans(&mut all, IDLE_GAP_CAP_MS);
        assert_eq!(incremental, full);
    }

    #[test]
    fn project_bursts_splits_cleanly_across_local_midnight() {
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let day0 = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let start = offset
            .with_ymd_and_hms(2026, 11, 13, 23, 30, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let end = offset
            .with_ymd_and_hms(2026, 11, 14, 0, 15, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let p1 = offset
            .with_ymd_and_hms(2026, 11, 13, 23, 45, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let p2 = offset
            .with_ymd_and_hms(2026, 11, 14, 0, 5, 0)
            .single()
            .unwrap()
            .timestamp_millis();

        let bursts =
            project_bursts_into_days(&[RawSpan::clamped(start, end)], &[p1, p2], day0, 2, offset);
        assert_eq!(bursts.len(), 2);
        assert_eq!(bursts[0].0, 0);
        assert!((bursts[0].1 - 23.5).abs() < 1e-4);
        assert!((bursts[0].2 - 24.0).abs() < 1e-4);
        assert_eq!(bursts[0].3, vec![23.75]);
        assert_eq!(bursts[0].4, 30);

        assert_eq!(bursts[1].0, 1);
        assert!((bursts[1].1 - 0.0).abs() < 1e-4);
        assert!((bursts[1].2 - 0.25).abs() < 1e-4);
        assert!((bursts[1].3[0] - (5.0 / 60.0)).abs() < 1e-3);
        assert_eq!(bursts[1].4, 15);
    }
}
