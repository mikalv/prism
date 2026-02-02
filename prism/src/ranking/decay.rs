//! Time-based decay functions for recency scoring
//!
//! These functions adjust document scores based on their age, making
//! newer documents rank higher than older ones.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Decay function types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecayFunction {
    /// Exponential decay: decay_rate^(age/scale)
    Exponential,
    /// Linear decay: max(0, 1 - age/scale)
    Linear,
    /// Gaussian decay: exp(-0.5 * (age/scale)^2 * ln(decay_rate))
    Gaussian,
}

impl DecayFunction {
    /// Parse decay function from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "linear" => DecayFunction::Linear,
            "gauss" | "gaussian" => DecayFunction::Gaussian,
            _ => DecayFunction::Exponential, // default
        }
    }
}

/// Configuration for recency decay
#[derive(Debug, Clone)]
pub struct DecayConfig {
    pub function: DecayFunction,
    /// Time scale as duration
    pub scale: Duration,
    /// Optional offset before decay starts
    pub offset: Option<Duration>,
    /// Decay rate (0.0 to 1.0) - score at scale distance
    pub decay_rate: f64,
}

impl DecayConfig {
    /// Create a new decay config
    pub fn new(function: DecayFunction, scale: Duration, decay_rate: f64) -> Self {
        Self {
            function,
            scale,
            offset: None,
            decay_rate,
        }
    }

    /// Set offset before decay starts
    pub fn with_offset(mut self, offset: Duration) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// Compute exponential decay multiplier
///
/// Formula: decay_rate^(age/scale)
///
/// At age=0: returns 1.0
/// At age=scale: returns decay_rate
/// At age=2*scale: returns decay_rate^2
pub fn exponential_decay(age_secs: f64, scale_secs: f64, decay_rate: f64) -> f64 {
    if scale_secs <= 0.0 {
        return 1.0;
    }
    decay_rate.powf(age_secs / scale_secs)
}

/// Compute linear decay multiplier
///
/// Formula: max(0, 1 - age/scale)
///
/// At age=0: returns 1.0
/// At age=scale: returns 0.0
/// Beyond scale: returns 0.0
pub fn linear_decay(age_secs: f64, scale_secs: f64, _decay_rate: f64) -> f64 {
    if scale_secs <= 0.0 {
        return 1.0;
    }
    (1.0 - age_secs / scale_secs).max(0.0)
}

/// Compute gaussian decay multiplier
///
/// Formula: exp(-0.5 * (age/scale)^2 * |ln(decay_rate)|)
///
/// Produces a bell-curve shaped decay that's gentler than exponential
/// near the origin but still drops smoothly.
pub fn gaussian_decay(age_secs: f64, scale_secs: f64, decay_rate: f64) -> f64 {
    if scale_secs <= 0.0 {
        return 1.0;
    }
    let normalized = age_secs / scale_secs;
    (-0.5 * normalized.powi(2) * decay_rate.ln().abs()).exp()
}

/// Compute decay multiplier based on configuration
pub fn compute_decay(config: &DecayConfig, document_time: SystemTime, now: SystemTime) -> f64 {
    // Calculate age in seconds
    let age = now.duration_since(document_time).unwrap_or(Duration::ZERO);

    // Apply offset if configured
    let effective_age = if let Some(offset) = config.offset {
        if age <= offset {
            return 1.0; // No decay within offset period
        }
        age - offset
    } else {
        age
    };

    let age_secs = effective_age.as_secs_f64();
    let scale_secs = config.scale.as_secs_f64();

    match config.function {
        DecayFunction::Exponential => exponential_decay(age_secs, scale_secs, config.decay_rate),
        DecayFunction::Linear => linear_decay(age_secs, scale_secs, config.decay_rate),
        DecayFunction::Gaussian => gaussian_decay(age_secs, scale_secs, config.decay_rate),
    }
}

/// Compute decay from a Unix timestamp in microseconds
pub fn compute_decay_from_micros(
    config: &DecayConfig,
    timestamp_micros: i64,
    now: SystemTime,
) -> f64 {
    let document_time = UNIX_EPOCH + Duration::from_micros(timestamp_micros as u64);
    compute_decay(config, document_time, now)
}

/// Parse duration from string like "7d", "30d", "1h", "2w"
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = if s.ends_with('d') {
        (&s[..s.len() - 1], "d")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else if s.ends_with('w') {
        (&s[..s.len() - 1], "w")
    } else if s.ends_with('m') && !s.ends_with("ms") {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else {
        // Assume days if no unit
        (s, "d")
    };

    let num: f64 = num_str.parse().ok()?;

    let secs = match unit {
        "s" => num,
        "m" => num * 60.0,
        "h" => num * 3600.0,
        "d" => num * 86400.0,
        "w" => num * 604800.0,
        _ => num * 86400.0, // default to days
    };

    Some(Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_decay() {
        // At age=0, should return 1.0
        assert!((exponential_decay(0.0, 86400.0, 0.5) - 1.0).abs() < 0.001);

        // At age=scale (1 day), should return decay_rate
        assert!((exponential_decay(86400.0, 86400.0, 0.5) - 0.5).abs() < 0.001);

        // At age=2*scale, should return decay_rate^2
        assert!((exponential_decay(172800.0, 86400.0, 0.5) - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_linear_decay() {
        // At age=0, should return 1.0
        assert!((linear_decay(0.0, 86400.0, 0.5) - 1.0).abs() < 0.001);

        // At age=scale/2, should return 0.5
        assert!((linear_decay(43200.0, 86400.0, 0.5) - 0.5).abs() < 0.001);

        // At age=scale, should return 0.0
        assert!((linear_decay(86400.0, 86400.0, 0.5) - 0.0).abs() < 0.001);

        // Beyond scale, should return 0.0
        assert!((linear_decay(172800.0, 86400.0, 0.5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_gaussian_decay() {
        // At age=0, should return 1.0
        assert!((gaussian_decay(0.0, 86400.0, 0.5) - 1.0).abs() < 0.001);

        // Should be higher than exponential at same age (gentler near origin)
        let exp = exponential_decay(43200.0, 86400.0, 0.5);
        let gauss = gaussian_decay(43200.0, 86400.0, 0.5);
        assert!(gauss > exp);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(7 * 86400)));
        assert_eq!(parse_duration("24h"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("1w"), Some(Duration::from_secs(604800)));
        assert_eq!(parse_duration("30"), Some(Duration::from_secs(30 * 86400))); // default to days
        assert!(parse_duration("").is_none());
    }

    #[test]
    fn test_decay_with_offset() {
        let config = DecayConfig::new(DecayFunction::Exponential, Duration::from_secs(86400), 0.5)
            .with_offset(Duration::from_secs(3600)); // 1 hour offset

        let now = SystemTime::now();

        // Document indexed 30 minutes ago - within offset, no decay
        let recent = now - Duration::from_secs(1800);
        assert!((compute_decay(&config, recent, now) - 1.0).abs() < 0.001);

        // Document indexed 2 hours ago - 1 hour effective age after offset
        let older = now - Duration::from_secs(7200);
        let decay = compute_decay(&config, older, now);
        assert!(decay < 1.0 && decay > 0.5);
    }
}
