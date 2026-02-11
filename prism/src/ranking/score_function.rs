//! Score function reranker — expression-based scoring
//!
//! Evaluates simple arithmetic expressions like `_score * popularity * 0.01`
//! or `_score + log(likes + 1)` against document fields.

use crate::backends::SearchResult;
use crate::ranking::reranker::Reranker;
use async_trait::async_trait;

/// A reranker that evaluates a score expression against document fields.
///
/// Supported tokens:
/// - `_score` — the original search score
/// - Numeric field names — extracted from document fields
/// - Numeric literals (integer and float)
/// - Operators: `+`, `-`, `*`, `/`
/// - `log(expr)` — natural logarithm
/// - Parentheses for grouping
pub struct ScoreFunctionReranker {
    expression: String,
    tokens: Vec<Token>,
}

impl ScoreFunctionReranker {
    /// Create a new ScoreFunctionReranker from an expression string.
    pub fn new(expression: &str) -> anyhow::Result<Self> {
        let tokens = tokenize(expression)?;
        Ok(Self {
            expression: expression.to_string(),
            tokens,
        })
    }

    /// Evaluate the expression for a single document
    pub fn evaluate(
        &self,
        score: f32,
        fields: &std::collections::HashMap<String, serde_json::Value>,
    ) -> f32 {
        let mut parser = ExprParser::new(&self.tokens, score, fields);
        match parser.parse_expr() {
            Ok(val) => {
                if val.is_finite() {
                    val
                } else {
                    score // fallback on NaN/Inf
                }
            }
            Err(_) => score, // fallback on parse error
        }
    }
}

#[async_trait]
impl Reranker for ScoreFunctionReranker {
    async fn rerank(&self, _query: &str, documents: &[&str]) -> anyhow::Result<Vec<f32>> {
        // Without document fields, we can only return the original implicit scores
        // (score_function reranker is designed to work with rerank_results)
        Ok(documents.iter().map(|_| 0.0).collect())
    }

    async fn rerank_results(
        &self,
        _query: &str,
        results: &[SearchResult],
        _text_fields: &[String],
    ) -> anyhow::Result<Vec<f32>> {
        Ok(results
            .iter()
            .map(|r| self.evaluate(r.score, &r.fields))
            .collect())
    }

    fn name(&self) -> &str {
        "score_function"
    }
}

// ============================================================================
// Tokenizer
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f32),
    Ident(String), // field names and _score
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
    Func(String), // "log"
}

fn tokenize(expr: &str) -> anyhow::Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                i += 1;
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                // Determine if this is a negative number or subtraction
                let is_unary = tokens.is_empty()
                    || matches!(
                        tokens.last(),
                        Some(Token::Plus)
                            | Some(Token::Minus)
                            | Some(Token::Star)
                            | Some(Token::Slash)
                            | Some(Token::LParen)
                            | Some(Token::Comma)
                    );
                if is_unary && i + 1 < chars.len() && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '.') {
                    // Parse negative number
                    let start = i;
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                        i += 1;
                    }
                    let num_str: String = chars[start..i].iter().collect();
                    let num: f32 = num_str.parse().map_err(|_| {
                        anyhow::anyhow!("Invalid number: {}", num_str)
                    })?;
                    tokens.push(Token::Number(num));
                } else {
                    tokens.push(Token::Minus);
                    i += 1;
                }
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num: f32 = num_str
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid number: {}", num_str))?;
                tokens.push(Token::Number(num));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                // Check if next non-whitespace char is '(' => function call
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '(' && ident == "log" {
                    tokens.push(Token::Func(ident));
                } else {
                    tokens.push(Token::Ident(ident));
                }
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected character '{}' in expression",
                    ch
                ));
            }
        }
    }

    Ok(tokens)
}

// ============================================================================
// Recursive descent parser
// ============================================================================

struct ExprParser<'a> {
    tokens: &'a [Token],
    pos: usize,
    score: f32,
    fields: &'a std::collections::HashMap<String, serde_json::Value>,
}

impl<'a> ExprParser<'a> {
    fn new(
        tokens: &'a [Token],
        score: f32,
        fields: &'a std::collections::HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            tokens,
            pos: 0,
            score,
            fields,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    // expr = term (('+' | '-') term)*
    fn parse_expr(&mut self) -> anyhow::Result<f32> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    left += self.parse_term()?;
                }
                Some(Token::Minus) => {
                    self.advance();
                    left -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // term = factor (('*' | '/') factor)*
    fn parse_term(&mut self) -> anyhow::Result<f32> {
        let mut left = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.advance();
                    left *= self.parse_factor()?;
                }
                Some(Token::Slash) => {
                    self.advance();
                    let right = self.parse_factor()?;
                    if right == 0.0 {
                        left = 0.0; // avoid division by zero
                    } else {
                        left /= right;
                    }
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // factor = number | ident | func '(' expr ')' | '(' expr ')'
    fn parse_factor(&mut self) -> anyhow::Result<f32> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.advance();
                Ok(n)
            }
            Some(Token::Ident(ref name)) => {
                let name = name.clone();
                self.advance();
                if name == "_score" {
                    Ok(self.score)
                } else {
                    // Look up field value
                    Ok(self
                        .fields
                        .get(&name)
                        .and_then(|v| {
                            v.as_f64()
                                .or_else(|| v.as_i64().map(|i| i as f64))
                                .or_else(|| v.as_u64().map(|u| u as f64))
                        })
                        .unwrap_or(0.0) as f32)
                }
            }
            Some(Token::Func(ref fname)) => {
                let fname = fname.clone();
                self.advance();
                // Expect '('
                match self.advance() {
                    Some(Token::LParen) => {}
                    _ => return Err(anyhow::anyhow!("Expected '(' after function {}", fname)),
                }
                let arg = self.parse_expr()?;
                // Expect ')'
                match self.advance() {
                    Some(Token::RParen) => {}
                    _ => return Err(anyhow::anyhow!("Expected ')' after function argument")),
                }
                match fname.as_str() {
                    "log" => Ok(arg.ln()),
                    _ => Err(anyhow::anyhow!("Unknown function: {}", fname)),
                }
            }
            Some(Token::LParen) => {
                self.advance();
                let val = self.parse_expr()?;
                match self.advance() {
                    Some(Token::RParen) => {}
                    _ => return Err(anyhow::anyhow!("Expected ')'")),
                }
                Ok(val)
            }
            other => Err(anyhow::anyhow!("Unexpected token: {:?}", other)),
        }
    }
}

impl std::fmt::Display for ScoreFunctionReranker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScoreFunctionReranker({})", self.expression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn make_result(score: f32, fields: HashMap<String, serde_json::Value>) -> SearchResult {
        SearchResult {
            id: "test".to_string(),
            score,
            fields,
            highlight: None,
        }
    }

    #[test]
    fn test_simple_score() {
        let reranker = ScoreFunctionReranker::new("_score").unwrap();
        let result = make_result(2.5, HashMap::new());
        assert!((reranker.evaluate(2.5, &result.fields) - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_score_multiply() {
        let reranker = ScoreFunctionReranker::new("_score * 2").unwrap();
        let result = make_result(3.0, HashMap::new());
        assert!((reranker.evaluate(3.0, &result.fields) - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_score_with_field() {
        let reranker = ScoreFunctionReranker::new("_score * popularity").unwrap();
        let fields = HashMap::from([("popularity".to_string(), json!(10.0))]);
        let result = make_result(1.5, fields);
        assert!((reranker.evaluate(1.5, &result.fields) - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_complex_expression() {
        let reranker = ScoreFunctionReranker::new("_score * popularity * 0.01").unwrap();
        let fields = HashMap::from([("popularity".to_string(), json!(500))]);
        let result = make_result(2.0, fields);
        // 2.0 * 500 * 0.01 = 10.0
        assert!((reranker.evaluate(2.0, &result.fields) - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_addition() {
        let reranker = ScoreFunctionReranker::new("_score + 1").unwrap();
        let result = make_result(2.0, HashMap::new());
        assert!((reranker.evaluate(2.0, &result.fields) - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_log_function() {
        let reranker = ScoreFunctionReranker::new("_score + log(likes + 1)").unwrap();
        let fields = HashMap::from([("likes".to_string(), json!(99))]);
        let result = make_result(1.0, fields);
        // 1.0 + ln(100) ≈ 1.0 + 4.605 ≈ 5.605
        let val = reranker.evaluate(1.0, &result.fields);
        assert!((val - 5.605).abs() < 0.01);
    }

    #[test]
    fn test_parentheses() {
        let reranker = ScoreFunctionReranker::new("(_score + 1) * 2").unwrap();
        let result = make_result(3.0, HashMap::new());
        assert!((reranker.evaluate(3.0, &result.fields) - 8.0).abs() < 0.001);
    }

    #[test]
    fn test_division() {
        let reranker = ScoreFunctionReranker::new("_score / 2").unwrap();
        let result = make_result(6.0, HashMap::new());
        assert!((reranker.evaluate(6.0, &result.fields) - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_division_by_zero() {
        let reranker = ScoreFunctionReranker::new("_score / 0").unwrap();
        let result = make_result(6.0, HashMap::new());
        // Should not panic, returns 0.0
        assert!((reranker.evaluate(6.0, &result.fields) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_missing_field_defaults_to_zero() {
        let reranker = ScoreFunctionReranker::new("_score + missing_field").unwrap();
        let result = make_result(2.0, HashMap::new());
        assert!((reranker.evaluate(2.0, &result.fields) - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_subtraction() {
        let reranker = ScoreFunctionReranker::new("_score - 0.5").unwrap();
        let result = make_result(2.0, HashMap::new());
        assert!((reranker.evaluate(2.0, &result.fields) - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_operator_precedence() {
        let reranker = ScoreFunctionReranker::new("1 + 2 * 3").unwrap();
        let result = make_result(0.0, HashMap::new());
        // Should be 1 + (2*3) = 7, not (1+2)*3 = 9
        assert!((reranker.evaluate(0.0, &result.fields) - 7.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_rerank_results() {
        let reranker = ScoreFunctionReranker::new("_score * 2").unwrap();

        let results = vec![
            make_result(1.0, HashMap::new()),
            make_result(2.0, HashMap::new()),
            make_result(3.0, HashMap::new()),
        ];

        let scores = reranker.rerank_results("query", &results, &[]).await.unwrap();
        assert_eq!(scores.len(), 3);
        assert!((scores[0] - 2.0).abs() < 0.001);
        assert!((scores[1] - 4.0).abs() < 0.001);
        assert!((scores[2] - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_negative_number() {
        let reranker = ScoreFunctionReranker::new("_score + -1").unwrap();
        let result = make_result(5.0, HashMap::new());
        assert!((reranker.evaluate(5.0, &result.fields) - 4.0).abs() < 0.001);
    }
}
