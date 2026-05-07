//! Pure usage-math helpers shared by the refresh loop and the renderer.
//! Ported from the Electron-era `lib/widget-core.js` so the same invariants
//! are testable on the Rust side.
#![allow(dead_code)]

pub fn normalize_display_mode(mode: &str) -> String {
    match mode.to_ascii_lowercase().as_str() {
        "left" => "left".into(),
        _ => "used".into(),
    }
}

pub fn mode_label(mode: &str) -> String {
    normalize_display_mode(mode).to_ascii_uppercase()
}

pub fn clamp_percent(value: Option<f64>) -> u32 {
    let Some(v) = value else { return 0 };
    if !v.is_finite() {
        return 0;
    }
    v.round().clamp(0.0, 100.0) as u32
}

pub fn compute_display_percent(used_percent: Option<f64>, mode: &str) -> u32 {
    let used = clamp_percent(used_percent);
    if normalize_display_mode(mode) == "left" {
        100 - used
    } else {
        used
    }
}

pub fn sanitize_thresholds(thresholds: Vec<u32>) -> Vec<u32> {
    let mut filtered: Vec<u32> = thresholds
        .into_iter()
        .filter(|t| (1..=100).contains(t))
        .collect();
    filtered.sort_unstable();
    filtered.dedup();
    filtered
}

pub fn sanitize_thresholds_with_default(thresholds: Vec<u32>) -> Vec<u32> {
    let cleaned = sanitize_thresholds(thresholds);
    if cleaned.is_empty() {
        vec![30, 60, 80, 90]
    } else {
        cleaned
    }
}

pub fn crossed_thresholds(previous: Option<f64>, current: Option<f64>, thresholds: &[u32]) -> Vec<u32> {
    let (Some(p), Some(c)) = (previous, current) else {
        return Vec::new();
    };
    if !p.is_finite() || !c.is_finite() {
        return Vec::new();
    }
    thresholds
        .iter()
        .copied()
        .filter(|t| p < f64::from(*t) && c >= f64::from(*t))
        .collect()
}

pub fn did_usage_window_reset(previous: Option<f64>, current: Option<f64>) -> bool {
    let (Some(p), Some(c)) = (previous, current) else {
        return false;
    };
    if !p.is_finite() || !c.is_finite() {
        return false;
    }
    c + 5.0 < p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_display_mode_falls_back_to_used() {
        assert_eq!(normalize_display_mode("left"), "left");
        assert_eq!(normalize_display_mode("LEFT"), "left");
        assert_eq!(normalize_display_mode("used"), "used");
        assert_eq!(normalize_display_mode(""), "used");
        assert_eq!(normalize_display_mode("garbage"), "used");
    }

    #[test]
    fn mode_label_uppercases() {
        assert_eq!(mode_label("left"), "LEFT");
        assert_eq!(mode_label("used"), "USED");
        assert_eq!(mode_label("garbage"), "USED");
    }

    #[test]
    fn clamp_percent_handles_edges() {
        assert_eq!(clamp_percent(None), 0);
        assert_eq!(clamp_percent(Some(f64::NAN)), 0);
        assert_eq!(clamp_percent(Some(-5.0)), 0);
        assert_eq!(clamp_percent(Some(0.0)), 0);
        assert_eq!(clamp_percent(Some(33.4)), 33);
        assert_eq!(clamp_percent(Some(33.6)), 34);
        assert_eq!(clamp_percent(Some(101.0)), 100);
    }

    #[test]
    fn compute_display_percent_inverts_for_left() {
        assert_eq!(compute_display_percent(Some(20.0), "used"), 20);
        assert_eq!(compute_display_percent(Some(20.0), "left"), 80);
        assert_eq!(compute_display_percent(None, "left"), 100);
        assert_eq!(compute_display_percent(Some(120.0), "used"), 100);
    }

    #[test]
    fn sanitize_thresholds_clamps_dedups_sorts() {
        assert_eq!(
            sanitize_thresholds(vec![80, 30, 60, 80, 0, 101, 90]),
            vec![30, 60, 80, 90]
        );
        assert_eq!(sanitize_thresholds(vec![]), Vec::<u32>::new());
        assert_eq!(sanitize_thresholds(vec![100, 1]), vec![1, 100]);
    }

    #[test]
    fn sanitize_thresholds_with_default_falls_back() {
        assert_eq!(
            sanitize_thresholds_with_default(vec![]),
            vec![30, 60, 80, 90]
        );
        assert_eq!(
            sanitize_thresholds_with_default(vec![0, 101]),
            vec![30, 60, 80, 90]
        );
        assert_eq!(
            sanitize_thresholds_with_default(vec![50, 75]),
            vec![50, 75]
        );
    }

    #[test]
    fn crossed_thresholds_returns_just_crossings() {
        assert_eq!(
            crossed_thresholds(Some(25.0), Some(65.0), &[30, 60, 80, 90]),
            vec![30, 60]
        );
        assert_eq!(
            crossed_thresholds(Some(60.0), Some(60.0), &[60]),
            Vec::<u32>::new()
        );
        assert_eq!(
            crossed_thresholds(Some(59.9), Some(60.0), &[60]),
            vec![60]
        );
        assert_eq!(
            crossed_thresholds(None, Some(95.0), &[30, 60, 90]),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn did_usage_window_reset_detects_meaningful_drops() {
        assert!(did_usage_window_reset(Some(80.0), Some(2.0)));
        assert!(!did_usage_window_reset(Some(80.0), Some(78.0)));
        assert!(!did_usage_window_reset(Some(80.0), Some(76.0))); // 4 < 5 threshold
        assert!(did_usage_window_reset(Some(80.0), Some(74.0))); // 6 ≥ 5
        assert!(!did_usage_window_reset(None, Some(0.0)));
        assert!(!did_usage_window_reset(Some(f64::NAN), Some(0.0)));
    }
}
