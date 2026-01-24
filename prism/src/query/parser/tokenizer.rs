use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_until, take_while1},
    character::complete::{char, multispace0},
    combinator::map,
    multi::many0,
    sequence::{delimited, preceded, separated_pair},
    IResult,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Term(String),
    QuotedString(String),
    FieldName(String),
    Colon,
    And,
    Or,
    Not,
    Minus,
    LParen,
    RParen,
    Caret(f32),
    Wildcard(String),
}

/// Parse a simple term (alphanumeric + _ + -)
fn term(input: &str) -> IResult<&str, String> {
    map(
        take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-'),
        |s: &str| s.to_string(),
    )(input)
}

/// Parse a quoted string
fn quoted_string(input: &str) -> IResult<&str, String> {
    delimited(
        char('"'),
        map(take_until("\""), |s: &str| s.to_string()),
        char('"'),
    )(input)
}

/// Parse field:value
fn field_term(input: &str) -> IResult<&str, (String, String)> {
    separated_pair(term, char(':'), alt((quoted_string, term)))(input)
}

/// Parse boolean operators
fn operator(input: &str) -> IResult<&str, Token> {
    alt((
        map(tag_no_case("AND"), |_| Token::And),
        map(tag_no_case("OR"), |_| Token::Or),
        map(tag_no_case("NOT"), |_| Token::Not),
        map(char('-'), |_| Token::Minus),
    ))(input)
}

pub fn tokenize(input: &str) -> IResult<&str, Vec<Token>> {
    many0(preceded(
        multispace0,
        alt((
            operator,
            map(field_term, |(f, v)| {
                Token::FieldName(format!("{}:{}", f, v))
            }),
            map(quoted_string, Token::QuotedString),
            map(term, Token::Term),
        )),
    ))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple() {
        let (_, tokens) = tokenize("auth bug").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], Token::Term("auth".to_string()));
        assert_eq!(tokens[1], Token::Term("bug".to_string()));
    }

    #[test]
    fn test_tokenize_boolean() {
        let (_, tokens) = tokenize("auth AND bug").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[1], Token::And);
    }

    #[test]
    fn test_tokenize_quoted() {
        let (_, tokens) = tokenize("\"auth bug\"").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], Token::QuotedString("auth bug".to_string()));
    }
}
