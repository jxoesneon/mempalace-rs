//! dynamics.rs — Living-connection math for memory records.
//!
//! This is a heuristic Rust port of the upstream Python `dynamics.py` module.
//! It is pure math with no I/O and no LLM calls. It operates on plain
//! numeric fields (`strength`, `stability`, `last_accessed`, `access_count`)
//! and can be used to score, decay, boost, and prune memory records.
//!
//! The model combines three pieces of memory research:
//!
//! * Hebbian potentiation: co-access strengthens a memory connection.
//! * Ebbinghaus exponential decay: strength fades with time since last use.
//! * Cepeda spacing effect: stability (decay resistance) grows when
//!   reinforcements are spaced, not massed.

use std::cmp::Ordering;

/// Seconds in one hour.
pub const SECONDS_PER_HOUR: f64 = 3600.0;
/// Seconds in one day.
pub const SECONDS_PER_DAY: f64 = 86400.0;

/// Lower bound on strength. Memories are never decayed to zero salience.
pub const STRENGTH_FLOOR: f64 = 0.05;
/// Upper bound on strength so extremely hot memories don't dominate forever.
pub const MAX_STRENGTH: f64 = 5.0;
/// Initial stability for a newly created memory.
pub const DEFAULT_STABILITY: f64 = 1.0;
/// Initial strength for a newly created memory.
pub const DEFAULT_STRENGTH: f64 = 1.0;
/// Default strength increase on each co-access event.
pub const POTENTIATION_INCREMENT: f64 = 0.05;
/// Minimum gap (in hours) between potentiations to count as spaced reinforcement.
pub const SPACED_INTERVAL_HOURS: f64 = 1.0;
/// How much stability grows on each spaced reinforcement.
pub const STABILITY_INCREMENT: f64 = 0.1;

/// Half-life (in days) for the recency boost.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 7.0;
/// Maximum extra boost multiplier for recency (boost ranges from 1.0 to 1.5).
pub const RECENCY_MAX_BOOST: f64 = 0.5;

/// Default threshold (in days) for considering a memory stale.
pub const DEFAULT_STALE_THRESHOLD_DAYS: f64 = 90.0;

/// Snapshot of a memory's dynamic state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryDynamics {
    /// Hebbian connection weight / importance.
    pub strength: f64,
    /// Decay resistance; higher is slower decay.
    pub stability: f64,
    /// Unix timestamp of the last activation.
    pub last_activated: i64,
    /// Cumulative access events.
    pub access_count: u64,
    /// Creation timestamp, used as a fallback for `last_activated`.
    pub created_at: i64,
}

impl Default for MemoryDynamics {
    fn default() -> Self {
        Self {
            strength: DEFAULT_STRENGTH,
            stability: DEFAULT_STABILITY,
            last_activated: 0,
            access_count: 0,
            created_at: 0,
        }
    }
}

impl MemoryDynamics {
    /// Create a fresh memory dynamics state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a dynamic state from the numeric fields stored in a memory record.
    pub fn from_memory(strength: f64, last_accessed: i64, access_count: u64) -> Self {
        let stability = DEFAULT_STABILITY + (access_count as f64 * STABILITY_INCREMENT);
        Self {
            strength,
            stability,
            last_activated: last_accessed,
            access_count,
            created_at: last_accessed,
        }
    }

    /// Ensure all dynamic fields are populated with safe defaults.
    ///
    /// Existing, non-zero values are preserved so this is safe to call on
    /// records that pre-date the dynamics fields.
    pub fn initialize(&mut self, now: i64) {
        if self.strength <= 0.0 || !self.strength.is_normal() {
            self.strength = DEFAULT_STRENGTH;
        }
        if self.stability <= 0.0 || !self.stability.is_normal() {
            self.stability = DEFAULT_STABILITY;
        }
        if self.last_activated == 0 {
            self.last_activated = if self.created_at > 0 {
                self.created_at
            } else {
                now
            };
        }
        if self.created_at == 0 {
            self.created_at = self.last_activated;
        }
    }

    /// Strengthen this memory on a co-access event.
    pub fn potentiate(&mut self, now: i64) {
        self.initialize(now);
        let hours_since = (now - self.last_activated) as f64 / SECONDS_PER_HOUR;
        if hours_since >= SPACED_INTERVAL_HOURS {
            self.stability += STABILITY_INCREMENT;
        }
        self.strength = (self.strength + POTENTIATION_INCREMENT).min(MAX_STRENGTH);
        self.last_activated = now;
        self.access_count += 1;
    }

    /// Apply Ebbinghaus exponential decay to the current strength.
    ///
    /// Updating `last_activated` to `now` makes repeated calls at the same
    /// instant idempotent.
    pub fn decay(&mut self, now: i64) {
        self.initialize(now);
        self.strength = decay_importance(self.strength, self.last_activated, self.stability, now);
        self.last_activated = now;
    }

    /// Compute a combined lifecycle score for this memory at time `now`.
    pub fn score(&self, now: i64) -> f64 {
        score_memory(self.strength, self.last_activated, self.access_count, now)
    }

    /// Recency boost multiplier for this memory at time `now`.
    pub fn recency_boost(&self, now: i64) -> f64 {
        boost_recent(1.0, self.last_activated, now)
    }

    /// Return true if the memory should be considered stale.
    pub fn is_stale(&self, now: i64, threshold_days: f64) -> bool {
        let days_since = days_since(self.last_activated, now);
        if days_since > threshold_days {
            return true;
        }
        self.score(now) <= STRENGTH_FLOOR
    }
}

/// Days since `last_accessed` relative to `now`, clamped to non-negative.
fn days_since(last_accessed: i64, now: i64) -> f64 {
    ((now - last_accessed) as f64 / SECONDS_PER_DAY).max(0.0)
}

/// Compute a lifecycle score combining decay, access frequency, and recency.
///
/// * `strength`: base importance or Hebbian weight.
/// * `last_accessed`: Unix timestamp of the last access.
/// * `access_count`: cumulative number of accesses.
/// * `now`: current Unix timestamp.
pub fn score_memory(strength: f64, last_accessed: i64, access_count: u64, now: i64) -> f64 {
    let stability = DEFAULT_STABILITY + (access_count as f64 * STABILITY_INCREMENT);
    let decayed = decay_importance(strength, last_accessed, stability, now);
    let freq = frequency_boost(access_count);
    let recency = boost_recent(1.0, last_accessed, now);
    decayed * freq * recency
}

/// Apply exponential decay to a base strength, floored at `STRENGTH_FLOOR`.
///
/// Returns `strength` unchanged when `last_accessed` is in the future (clock
/// skew) or `now` is the same instant, making the function idempotent.
pub fn decay_importance(strength: f64, last_accessed: i64, stability: f64, now: i64) -> f64 {
    let days_since = days_since(last_accessed, now);
    if days_since <= 0.0 {
        return strength.max(STRENGTH_FLOOR);
    }
    let stability = if stability > 0.0 && stability.is_normal() {
        stability
    } else {
        DEFAULT_STABILITY
    };
    let decayed = strength * (-days_since / stability).exp();
    decayed.max(STRENGTH_FLOOR)
}

/// Boost a score based on how recently it was accessed.
///
/// Recent memories get a multiplicative boost up to `1 + RECENCY_MAX_BOOST`;
/// the boost decays exponentially with a half-life of `RECENCY_HALF_LIFE_DAYS`.
pub fn boost_recent(strength: f64, last_accessed: i64, now: i64) -> f64 {
    let days_since = days_since(last_accessed, now);
    let multiplier = 1.0 + RECENCY_MAX_BOOST * (-days_since / RECENCY_HALF_LIFE_DAYS).exp();
    strength * multiplier
}

/// Frequency boost derived from cumulative access count.
///
/// Always returns at least `1.0` so zero-access memories are not zeroed out.
pub fn frequency_boost(access_count: u64) -> f64 {
    (1.0 + access_count as f64).ln().max(1.0)
}

/// Return a copy of the records that are stale according to `is_stale`.
///
/// Callers can use this to drive a pruning pass over their own storage.
pub fn prune_stale(
    records: &[MemoryDynamics],
    now: i64,
    threshold_days: f64,
) -> Vec<MemoryDynamics> {
    records
        .iter()
        .filter(|r| r.is_stale(now, threshold_days))
        .cloned()
        .collect()
}

/// Return the highest-scoring records first, up to `limit`.
///
/// `limit` is silently capped at 10,000 to avoid accidental huge allocations.
pub fn rank_by_score(
    records: &mut [MemoryDynamics],
    now: i64,
    limit: usize,
) -> Vec<MemoryDynamics> {
    records.sort_by(|a, b| {
        b.score(now)
            .partial_cmp(&a.score(now))
            .unwrap_or(Ordering::Equal)
    });
    let cap = limit.min(10_000);
    records.iter().take(cap).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    #[test]
    fn test_dynamics_default() {
        let d = MemoryDynamics::default();
        assert_eq!(d.strength, DEFAULT_STRENGTH);
        assert_eq!(d.stability, DEFAULT_STABILITY);
        assert_eq!(d.last_activated, 0);
        assert_eq!(d.access_count, 0);
    }

    #[test]
    fn test_dynamics_new() {
        let d = MemoryDynamics::new();
        assert_eq!(d.strength, DEFAULT_STRENGTH);
        assert_eq!(d.access_count, 0);
    }

    #[test]
    fn test_dynamics_from_memory() {
        let d = MemoryDynamics::from_memory(3.0, NOW, 5);
        assert_eq!(d.strength, 3.0);
        assert_eq!(d.last_activated, NOW);
        assert_eq!(d.access_count, 5);
        assert!(d.stability > DEFAULT_STABILITY);
    }

    #[test]
    fn test_dynamics_initialize_sets_defaults() {
        let mut d = MemoryDynamics {
            strength: 0.0,
            stability: 0.0,
            last_activated: 0,
            access_count: 0,
            created_at: 0,
        };
        d.initialize(NOW);
        assert_eq!(d.strength, DEFAULT_STRENGTH);
        assert_eq!(d.stability, DEFAULT_STABILITY);
        assert_eq!(d.last_activated, NOW);
        assert_eq!(d.created_at, NOW);
    }

    #[test]
    fn test_dynamics_initialize_preserves_existing_values() {
        let mut d = MemoryDynamics::from_memory(2.0, NOW - 100, 3);
        d.initialize(NOW);
        assert_eq!(d.strength, 2.0);
        assert_eq!(d.last_activated, NOW - 100);
        assert_eq!(d.access_count, 3);
    }

    #[test]
    fn test_dynamics_potentiate_basic() {
        let mut d = MemoryDynamics::new();
        d.potentiate(NOW);
        assert_eq!(d.access_count, 1);
        assert_eq!(d.last_activated, NOW);
        assert!(d.strength > DEFAULT_STRENGTH);
        assert!(d.strength <= MAX_STRENGTH);
    }

    #[test]
    fn test_dynamics_potentiate_spaced_increases_stability() {
        let mut d = MemoryDynamics::new();
        d.potentiate(NOW);
        let before = d.stability;
        d.potentiate(NOW + (SECONDS_PER_HOUR * 2.0) as i64);
        assert_eq!(d.stability, before + STABILITY_INCREMENT);
        assert_eq!(d.access_count, 2);
    }

    #[test]
    fn test_dynamics_potentiate_massed_does_not_increase_stability() {
        let mut d = MemoryDynamics::new();
        d.potentiate(NOW);
        let before = d.stability;
        d.potentiate(NOW + 1);
        assert_eq!(d.stability, before);
        assert_eq!(d.access_count, 2);
    }

    #[test]
    fn test_dynamics_potentiate_caps_strength() {
        let mut d = MemoryDynamics {
            strength: MAX_STRENGTH,
            stability: DEFAULT_STABILITY,
            last_activated: NOW - (SECONDS_PER_HOUR * 2.0) as i64,
            access_count: 0,
            created_at: NOW - (SECONDS_PER_HOUR * 2.0) as i64,
        };
        d.potentiate(NOW);
        assert_eq!(d.strength, MAX_STRENGTH);
    }

    #[test]
    fn test_dynamics_decay_basic() {
        let mut d = MemoryDynamics::from_memory(2.0, NOW - SECONDS_PER_DAY as i64, 0);
        d.decay(NOW);
        assert!(d.strength < 2.0);
        assert!(d.strength >= STRENGTH_FLOOR);
    }

    #[test]
    fn test_dynamics_decay_idempotent_at_same_instant() {
        let mut d = MemoryDynamics::from_memory(2.0, NOW - SECONDS_PER_DAY as i64, 0);
        d.decay(NOW);
        let first = d.strength;
        d.decay(NOW);
        assert!((d.strength - first).abs() < 1e-9);
    }

    #[test]
    fn test_dynamics_decay_future_last_activated() {
        let strength = 2.0;
        let decayed = decay_importance(strength, NOW + 1000, DEFAULT_STABILITY, NOW);
        assert!((decayed - strength).abs() < 1e-9);
    }

    #[test]
    fn test_dynamics_decay_respects_stability() {
        let old = NOW - (SECONDS_PER_DAY * 30.0) as i64;
        let low_stability = decay_importance(2.0, old, 1.0, NOW);
        let high_stability = decay_importance(2.0, old, 10.0, NOW);
        assert!(high_stability > low_stability);
    }

    #[test]
    fn test_dynamics_decay_importance_positive() {
        let s = decay_importance(5.0, NOW, DEFAULT_STABILITY, NOW);
        assert!(s > 0.0);
    }

    #[test]
    fn test_dynamics_score_memory_positive() {
        let score = score_memory(5.0, NOW, 1, NOW);
        assert!(score > 0.0);
    }

    #[test]
    fn test_dynamics_score_memory_recent_higher_than_old() {
        let recent = score_memory(5.0, NOW, 0, NOW);
        let old = score_memory(5.0, NOW - (SECONDS_PER_DAY * 60.0) as i64, 0, NOW);
        assert!(recent > old);
    }

    #[test]
    fn test_dynamics_score_memory_freq_boost() {
        let low_freq = score_memory(5.0, NOW, 0, NOW);
        let high_freq = score_memory(5.0, NOW, 10, NOW);
        assert!(high_freq > low_freq);
    }

    #[test]
    fn test_dynamics_boost_recent_multiplier() {
        let recent = boost_recent(1.0, NOW, NOW);
        let old = boost_recent(1.0, NOW - (SECONDS_PER_DAY * 30.0) as i64, NOW);
        assert!(recent > 1.0);
        assert!(recent > old);
        assert!(old >= 1.0);
    }

    #[test]
    fn test_dynamics_frequency_boost_minimum_one() {
        assert_eq!(frequency_boost(0), 1.0);
        assert!(frequency_boost(1) >= 1.0);
        assert!(frequency_boost(100) > frequency_boost(1));
        assert!(frequency_boost(100) > 1.0);
    }

    #[test]
    fn test_dynamics_is_stale_by_threshold() {
        let d = MemoryDynamics::from_memory(2.0, NOW - (SECONDS_PER_DAY * 100.0) as i64, 0);
        assert!(d.is_stale(NOW, DEFAULT_STALE_THRESHOLD_DAYS));
        assert!(!d.is_stale(NOW, 200.0));
    }

    #[test]
    fn test_dynamics_is_stale_by_low_score() {
        let d = MemoryDynamics::from_memory(0.01, NOW - (SECONDS_PER_DAY * 365.0) as i64, 0);
        assert!(d.is_stale(NOW, DEFAULT_STALE_THRESHOLD_DAYS));
    }

    #[test]
    fn test_dynamics_prune_stale() {
        let fresh = MemoryDynamics::from_memory(2.0, NOW, 5);
        let stale = MemoryDynamics::from_memory(2.0, NOW - (SECONDS_PER_DAY * 100.0) as i64, 0);
        let records = vec![fresh, stale];
        let pruned = prune_stale(&records, NOW, DEFAULT_STALE_THRESHOLD_DAYS);
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].last_activated, stale.last_activated);
    }

    #[test]
    fn test_dynamics_prune_stale_empty() {
        let pruned = prune_stale(&[], NOW, DEFAULT_STALE_THRESHOLD_DAYS);
        assert!(pruned.is_empty());
    }

    #[test]
    fn test_dynamics_rank_by_score() {
        let a = MemoryDynamics::from_memory(5.0, NOW, 10);
        let b = MemoryDynamics::from_memory(5.0, NOW - (SECONDS_PER_DAY * 30.0) as i64, 0);
        let c = MemoryDynamics::from_memory(1.0, NOW - (SECONDS_PER_DAY * 100.0) as i64, 0);
        let mut records = [c, a, b];
        let ranked = rank_by_score(&mut records, NOW, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].access_count, 10);
        assert_eq!(ranked[1].access_count, 0);
    }

    #[test]
    fn test_dynamics_score_memory_uses_stability_from_access_count() {
        let old = NOW - (SECONDS_PER_DAY * 30.0) as i64;
        let low_access = score_memory(5.0, old, 0, NOW);
        let high_access = score_memory(5.0, old, 100, NOW);
        // Higher access count -> higher stability -> less decay, plus freq boost.
        assert!(high_access > low_access);
    }

    #[test]
    fn test_dynamics_score_zero_access_count() {
        let score = score_memory(5.0, NOW, 0, NOW);
        assert!(score > 0.0);
    }

    #[test]
    fn test_dynamics_constants() {
        assert!(STRENGTH_FLOOR > 0.0);
        assert!(MAX_STRENGTH > DEFAULT_STRENGTH);
        assert!(STABILITY_INCREMENT > 0.0);
        assert!(SPACED_INTERVAL_HOURS > 0.0);
    }
}
