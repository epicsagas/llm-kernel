//! Extract structured JSON from raw LLM text output.
//!
//! LLMs often wrap JSON in markdown code fences or add prose around it.
//! This module handles the common cases:
//!
//! - ` ```json ... ``` ` fenced blocks
//! - ` ``` ... ``` ` fences with JSON inside
//! - Raw balanced-bracket JSON buried in text

use serde::de::DeserializeOwned;

/// Extract the first JSON object or array from raw LLM text.
///
/// Handles ```json fences, ``` fences, and raw JSON.
pub fn extract_json(text: &str) -> Option<String> {
    let text = text.trim();

    // Try ```json ... ```
    if let Some(extracted) = extract_fenced(text, "json") {
        return Some(extracted);
    }

    // Try ``` ... ``` (generic fence)
    if let Some(extracted) = extract_fenced(text, "")
        && looks_like_json(&extracted)
    {
        return Some(extracted);
    }

    // Try raw balanced bracket matching
    find_balanced_json(text)
}

/// Parse LLM output into a typed struct using `extract_json` + `serde_json`.
///
/// Returns a deserialization error if no JSON is found or parsing fails.
pub fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, ParseJsonError> {
    let json_str = extract_json(text).ok_or(ParseJsonError::NoJsonFound)?;
    serde_json::from_str(&json_str).map_err(ParseJsonError::Deserialize)
}

/// Error from [`parse_json`].
#[derive(Debug)]
pub enum ParseJsonError {
    /// No JSON found in the LLM output text.
    NoJsonFound,
    /// Found JSON but deserialization failed.
    Deserialize(serde_json::Error),
}

impl std::fmt::Display for ParseJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoJsonFound => write!(f, "no JSON found in LLM output"),
            Self::Deserialize(e) => write!(f, "JSON parse error: {e}"),
        }
    }
}

impl std::error::Error for ParseJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoJsonFound => None,
            Self::Deserialize(e) => Some(e),
        }
    }
}

/// Type-safe JSON extraction wrapper for use with LLM clients.
pub struct JsonExtractor<T>(std::marker::PhantomData<T>);

impl<T: DeserializeOwned> JsonExtractor<T> {
    /// Parse a raw LLM response string into `T`.
    pub fn parse(text: &str) -> Result<T, ParseJsonError> {
        parse_json(text)
    }
}

fn extract_fenced(text: &str, lang: &str) -> Option<String> {
    let opener = if lang.is_empty() {
        "```".to_string()
    } else {
        format!("```{}", lang)
    };

    let start = text.find(&opener)?;
    let after_open = start + opener.len();

    // Skip to end of opening line
    let body_start = text[after_open..].find('\n').map_or(after_open, |i| after_open + i + 1);

    // Find closing ```
    let closer = text[body_start..].find("```")?;
    let body = text[body_start..body_start + closer].trim();

    if body.is_empty() {
        return None;
    }

    Some(body.to_string())
}

fn looks_like_json(s: &str) -> bool {
    let trimmed = s.trim();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn find_balanced_json(text: &str) -> Option<String> {
    for (i, b) in text.bytes().enumerate() {
        if b == b'{' || b == b'[' {
            let open = b;
            let close = if open == b'{' { b'}' } else { b']' };
            if let Some(end) = find_balanced_end(text.as_bytes(), i, open, close) {
                return Some(text[i..=end].to_string());
            }
        }
    }
    None
}

fn find_balanced_end(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for (i, &b) in bytes[start..].iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(start + i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestOutput {
        name: String,
        value: i32,
    }

    #[test]
    fn extract_from_json_fence() {
        let input = "Here is the result:\n```json\n{\"name\":\"test\",\"value\":42}\n```\nDone.";
        let json = extract_json(input).unwrap();
        let parsed: TestOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TestOutput { name: "test".into(), value: 42 });
    }

    #[test]
    fn extract_from_generic_fence() {
        let input = "```\n{\"name\":\"x\",\"value\":1}\n```";
        let json = extract_json(input).unwrap();
        assert!(json.contains("\"name\""));
    }

    #[test]
    fn extract_raw_json_object() {
        let input = "The answer is {\"name\":\"raw\",\"value\":7} as shown.";
        let json = extract_json(input).unwrap();
        let parsed: TestOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TestOutput { name: "raw".into(), value: 7 });
    }

    #[test]
    fn extract_raw_json_array() {
        let input = "Items: [1, 2, 3]";
        let json = extract_json(input).unwrap();
        assert_eq!(json, "[1, 2, 3]");
    }

    #[test]
    fn no_json_returns_none() {
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn parse_json_works() {
        let input = "```json\n{\"name\":\"ok\",\"value\":99}\n```";
        let result: TestOutput = parse_json(input).unwrap();
        assert_eq!(result.value, 99);
    }

    #[test]
    fn parse_json_fails_gracefully() {
        let result: Result<TestOutput, ParseJsonError> = parse_json("no json");
        assert!(matches!(result, Err(ParseJsonError::NoJsonFound)));
    }

    #[test]
    fn nested_json() {
        let input = r#"{"outer": {"inner": [1,2]}}"#;
        let json = extract_json(input).unwrap();
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
    }

    #[test]
    fn escaped_quotes_in_string() {
        let input = r#"{"name":"he said \"hello\"","value":0}"#;
        let json = extract_json(input).unwrap();
        assert!(json.contains("hello"));
    }
}
