//! Tiny stats helpers for collecting percentiles without pulling in a
//! statistics crate. Used by the bench-runner CLI to turn raw sample
//! vectors into the publication-quality numbers documented in
//! `docs/benchmarks/*.md`.

use std::time::Duration;

/// Median + p95 + p99 of a set of durations. The input slice is sorted
/// in place. Panics if `samples` is empty.
pub fn percentiles(samples: &mut [Duration]) -> (Duration, Duration, Duration) {
    assert!(!samples.is_empty(), "percentiles: empty samples");
    samples.sort();
    let pick = |q: f64| -> Duration {
        let idx = ((samples.len() - 1) as f64 * q).round() as usize;
        samples[idx]
    };
    (pick(0.50), pick(0.95), pick(0.99))
}

/// Mean of a set of durations. Returns Duration::ZERO on empty input.
pub fn mean(samples: &[Duration]) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let total: Duration = samples.iter().sum();
    total / (samples.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_basic() {
        // 100 samples = indexes 0..=99. The quantile formula is
        // round((n-1) * q), so for n=100 we get index 50 (p50),
        // 94 (p95), 98 (p99), mapping to micros 51, 95, 99.
        let mut v: Vec<Duration> = (1..=100).map(|n| Duration::from_micros(n as u64)).collect();
        let (p50, p95, p99) = percentiles(&mut v);
        assert_eq!(p50, Duration::from_micros(51));
        assert_eq!(p95, Duration::from_micros(95));
        assert_eq!(p99, Duration::from_micros(99));
    }

    #[test]
    fn mean_basic() {
        let v = vec![
            Duration::from_micros(1),
            Duration::from_micros(2),
            Duration::from_micros(3),
        ];
        assert_eq!(mean(&v), Duration::from_micros(2));
    }
}
