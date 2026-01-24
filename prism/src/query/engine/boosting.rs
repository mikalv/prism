use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Recency decay function types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecayFunction {
    Exponential,
    Linear,
    Gaussian,
}

/// Calculate recency decay multiplier
///
/// # Arguments
/// * `timestamp` - Document timestamp
/// * `reference_time` - Current/reference time (usually now)
/// * `function` - Decay function type
/// * `scale` - Time scale for decay (e.g., 30 days)
/// * `offset` - Grace period before decay starts
/// * `decay_rate` - Decay rate parameter (for exponential)
///
/// # Returns
/// Multiplier in range [0.0, 1.0] where 1.0 = no decay (recent)
pub fn calculate_recency_decay(
    timestamp: DateTime<Utc>,
    reference_time: DateTime<Utc>,
    function: DecayFunction,
    scale: chrono::Duration,
    offset: chrono::Duration,
    decay_rate: f32,
) -> f32 {
    let age = reference_time.signed_duration_since(timestamp);

    // If within offset (grace period), no decay
    if age <= offset {
        return 1.0;
    }

    let effective_age = age - offset;
    let scale_seconds = scale.num_seconds() as f32;
    let age_seconds = effective_age.num_seconds() as f32;

    if age_seconds <= 0.0 {
        return 1.0;
    }

    match function {
        DecayFunction::Exponential => {
            // decay = exp(-decay_rate * age / scale)
            let exponent = -decay_rate * (age_seconds / scale_seconds);
            exponent.exp().clamp(0.0, 1.0)
        }
        DecayFunction::Linear => {
            // decay = max(0, 1 - age / scale)
            (1.0 - (age_seconds / scale_seconds)).clamp(0.0, 1.0)
        }
        DecayFunction::Gaussian => {
            // decay = exp(-(age / scale)^2)
            let ratio = age_seconds / scale_seconds;
            (-ratio * ratio).exp().clamp(0.0, 1.0)
        }
    }
}

/// Calculate context boost multiplier
///
/// # Arguments
/// * `document_context` - Document's context fields (e.g., project_id, session_id)
/// * `search_context` - Current search context values
/// * `boost_amount` - Multiplier to apply when context matches
///
/// # Returns
/// Multiplier where 1.0 = no boost, >1.0 = boosted for matching context
pub fn calculate_context_boost(
    document_context: &HashMap<String, String>,
    search_context: &HashMap<String, String>,
    boost_amount: f32,
) -> f32 {
    let mut multiplier = 1.0;

    for (key, search_value) in search_context {
        if let Some(doc_value) = document_context.get(key) {
            if doc_value == search_value {
                multiplier *= boost_amount;
            }
        }
    }

    multiplier
}

/// Calculate field weight boost
///
/// # Arguments
/// * `field_name` - Name of the field that matched
/// * `field_weights` - Map of field names to weight multipliers
///
/// # Returns
/// Weight multiplier for the field (default 1.0 if not specified)
pub fn calculate_field_weight(field_name: &str, field_weights: &HashMap<String, f32>) -> f32 {
    field_weights.get(field_name).copied().unwrap_or(1.0)
}

/// Apply all boosting factors to a base score
///
/// # Arguments
/// * `base_score` - Base relevance score from search
/// * `recency_multiplier` - Recency decay multiplier [0.0, 1.0]
/// * `context_multiplier` - Context boost multiplier [1.0, ∞)
/// * `field_weight` - Field weight multiplier
///
/// # Returns
/// Final boosted score
pub fn apply_boost(
    base_score: f32,
    recency_multiplier: f32,
    context_multiplier: f32,
    field_weight: f32,
) -> f32 {
    base_score * recency_multiplier * context_multiplier * field_weight
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_recency_no_decay_within_offset() {
        let now = Utc::now();
        let recent = now - Duration::hours(1);

        let multiplier = calculate_recency_decay(
            recent,
            now,
            DecayFunction::Exponential,
            Duration::days(30),
            Duration::days(1),
            0.5,
        );

        assert_eq!(multiplier, 1.0, "Should have no decay within offset");
    }

    #[test]
    fn test_exponential_decay() {
        let now = Utc::now();
        let old = now - Duration::days(30);

        let multiplier = calculate_recency_decay(
            old,
            now,
            DecayFunction::Exponential,
            Duration::days(30),
            Duration::days(0),
            0.5,
        );

        // After 1 scale period with decay_rate=0.5: exp(-0.5) ≈ 0.606
        assert!(
            multiplier > 0.5 && multiplier < 0.7,
            "Expected ~0.606, got {}",
            multiplier
        );
    }

    #[test]
    fn test_linear_decay() {
        let now = Utc::now();
        let half_old = now - Duration::days(15);

        let multiplier = calculate_recency_decay(
            half_old,
            now,
            DecayFunction::Linear,
            Duration::days(30),
            Duration::days(0),
            0.0,
        );

        // After half the scale period: 1 - 0.5 = 0.5
        assert!(
            (multiplier - 0.5).abs() < 0.01,
            "Expected ~0.5, got {}",
            multiplier
        );
    }

    #[test]
    fn test_gaussian_decay() {
        let now = Utc::now();
        let at_scale = now - Duration::days(30);

        let multiplier = calculate_recency_decay(
            at_scale,
            now,
            DecayFunction::Gaussian,
            Duration::days(30),
            Duration::days(0),
            0.0,
        );

        // At 1 scale: exp(-1) ≈ 0.368
        assert!(
            multiplier > 0.3 && multiplier < 0.4,
            "Expected ~0.368, got {}",
            multiplier
        );
    }

    #[test]
    fn test_very_old_document_decays_to_zero() {
        let now = Utc::now();
        let very_old = now - Duration::days(365);

        let multiplier = calculate_recency_decay(
            very_old,
            now,
            DecayFunction::Linear,
            Duration::days(30),
            Duration::days(0),
            0.0,
        );

        assert_eq!(multiplier, 0.0, "Very old documents should decay to 0");
    }
}

#[test]
fn test_context_boost_single_match() {
    let mut doc_context = HashMap::new();
    doc_context.insert("project_id".to_string(), "proj-123".to_string());

    let mut search_context = HashMap::new();
    search_context.insert("project_id".to_string(), "proj-123".to_string());

    let multiplier = calculate_context_boost(&doc_context, &search_context, 2.0);
    assert_eq!(multiplier, 2.0, "Should boost when context matches");
}

#[test]
fn test_context_boost_multiple_matches() {
    let mut doc_context = HashMap::new();
    doc_context.insert("project_id".to_string(), "proj-123".to_string());
    doc_context.insert("session_id".to_string(), "sess-456".to_string());

    let mut search_context = HashMap::new();
    search_context.insert("project_id".to_string(), "proj-123".to_string());
    search_context.insert("session_id".to_string(), "sess-456".to_string());

    let multiplier = calculate_context_boost(&doc_context, &search_context, 1.5);
    assert_eq!(multiplier, 2.25, "Should multiply boosts: 1.5 * 1.5 = 2.25");
}

#[test]
fn test_context_boost_no_match() {
    let mut doc_context = HashMap::new();
    doc_context.insert("project_id".to_string(), "proj-123".to_string());

    let mut search_context = HashMap::new();
    search_context.insert("project_id".to_string(), "proj-999".to_string());

    let multiplier = calculate_context_boost(&doc_context, &search_context, 2.0);
    assert_eq!(
        multiplier, 1.0,
        "Should not boost when context doesn't match"
    );
}

#[test]
fn test_field_weight_specified() {
    let mut weights = HashMap::new();
    weights.insert("title".to_string(), 2.0);
    weights.insert("content".to_string(), 1.0);

    let weight = calculate_field_weight("title", &weights);
    assert_eq!(weight, 2.0, "Should use specified weight");
}

#[test]
fn test_field_weight_default() {
    let weights = HashMap::new();

    let weight = calculate_field_weight("unknown_field", &weights);
    assert_eq!(weight, 1.0, "Should default to 1.0 for unknown fields");
}

#[test]
fn test_apply_boost_combined() {
    let base_score = 10.0;
    let recency = 0.8; // 80% recent
    let context = 1.5; // 50% context boost
    let field_weight = 2.0; // Title field

    let final_score = apply_boost(base_score, recency, context, field_weight);
    assert_eq!(final_score, 24.0, "Expected 10.0 * 0.8 * 1.5 * 2.0 = 24.0");
}

#[test]
fn test_apply_boost_no_boosts() {
    let base_score = 10.0;
    let final_score = apply_boost(base_score, 1.0, 1.0, 1.0);
    assert_eq!(
        final_score, 10.0,
        "Should preserve base score when all multipliers are 1.0"
    );
}
