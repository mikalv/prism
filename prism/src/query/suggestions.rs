//! Query suggestions using fuzzy matching

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr_row[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr_row[j] = std::cmp::min(
                std::cmp::min(curr_row[j - 1] + 1, prev_row[j] + 1),
                prev_row[j - 1] + cost,
            );
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[b_len]
}

/// Suggestion candidate with similarity score
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub term: String,
    pub score: f32,
}

/// Generate query suggestions based on fuzzy matching
///
/// # Arguments
/// * `query` - User's input query term
/// * `candidates` - Available terms to suggest from
/// * `max_distance` - Maximum Levenshtein distance to consider
/// * `limit` - Maximum number of suggestions to return
///
/// # Returns
/// Suggestions sorted by similarity score (highest first)
pub fn suggest_corrections(
    query: &str,
    candidates: &[String],
    max_distance: usize,
    limit: usize,
) -> Vec<Suggestion> {
    let query_lower = query.to_lowercase();
    let mut suggestions: Vec<Suggestion> = candidates
        .iter()
        .filter_map(|candidate| {
            let candidate_lower = candidate.to_lowercase();
            let distance = levenshtein_distance(&query_lower, &candidate_lower);

            if distance <= max_distance {
                // Score: 1.0 = exact match, decreases with distance
                let score =
                    1.0 - (distance as f32 / (query_lower.len().max(candidate_lower.len()) as f32));
                Some(Suggestion {
                    term: candidate.clone(),
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    // Sort by score descending
    suggestions.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    // Take top N
    suggestions.truncate(limit);
    suggestions
}

/// Generate suggestions for multi-word queries
///
/// Splits query into words and suggests corrections for each word
pub fn suggest_query_corrections(
    query: &str,
    vocabulary: &[String],
    max_distance: usize,
) -> Vec<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut corrected_words = Vec::new();

    for word in words {
        let suggestions = suggest_corrections(word, vocabulary, max_distance, 1);
        if let Some(best) = suggestions.first() {
            if best.score > 0.7 {
                // Only use suggestion if confidence is high
                corrected_words.push(best.term.clone());
            } else {
                corrected_words.push(word.to_string());
            }
        } else {
            corrected_words.push(word.to_string());
        }
    }

    if corrected_words.join(" ") == query {
        // No corrections made
        vec![]
    } else {
        vec![corrected_words.join(" ")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
        assert_eq!(levenshtein_distance("abc", "abd"), 1);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn test_suggest_corrections_exact_match() {
        let candidates = vec!["error".to_string(), "warning".to_string()];
        let suggestions = suggest_corrections("error", &candidates, 2, 5);

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].term, "error");
        assert_eq!(suggestions[0].score, 1.0);
    }

    #[test]
    fn test_suggest_corrections_fuzzy() {
        let candidates = vec![
            "error".to_string(),
            "warning".to_string(),
            "critical".to_string(),
        ];
        let suggestions = suggest_corrections("eror", &candidates, 2, 5);

        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].term, "error");
        assert!(suggestions[0].score > 0.5);
    }

    #[test]
    fn test_suggest_corrections_max_distance() {
        let candidates = vec!["error".to_string(), "warning".to_string()];
        let suggestions = suggest_corrections("xyz", &candidates, 1, 5);

        assert_eq!(
            suggestions.len(),
            0,
            "Should exclude suggestions beyond max distance"
        );
    }

    #[test]
    fn test_suggest_corrections_limit() {
        let candidates = vec![
            "error".to_string(),
            "errors".to_string(),
            "errored".to_string(),
        ];
        let suggestions = suggest_corrections("eror", &candidates, 3, 2);

        assert_eq!(suggestions.len(), 2, "Should respect limit");
    }

    #[test]
    fn test_suggest_corrections_sorted() {
        let candidates = vec![
            "warning".to_string(),
            "error".to_string(),
            "errors".to_string(),
        ];
        let suggestions = suggest_corrections("eror", &candidates, 3, 5);

        // "error" should be first (closer match than "errors")
        assert_eq!(suggestions[0].term, "error");
        if suggestions.len() > 1 {
            assert!(suggestions[0].score > suggestions[1].score);
        }
    }

    #[test]
    fn test_suggest_query_corrections_single_word() {
        let vocab = vec!["error".to_string(), "warning".to_string()];
        let suggestions = suggest_query_corrections("eror", &vocab, 2);

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0], "error");
    }

    #[test]
    fn test_suggest_query_corrections_multi_word() {
        let vocab = vec![
            "error".to_string(),
            "critical".to_string(),
            "warning".to_string(),
        ];
        let suggestions = suggest_query_corrections("eror critcal", &vocab, 2);

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0], "error critical");
    }

    #[test]
    fn test_suggest_query_corrections_no_change() {
        let vocab = vec!["error".to_string(), "warning".to_string()];
        let suggestions = suggest_query_corrections("error warning", &vocab, 2);

        assert_eq!(
            suggestions.len(),
            0,
            "Should return empty if no corrections made"
        );
    }
}
