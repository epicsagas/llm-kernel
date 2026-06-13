//! Prompt templates with variable substitution and few-shot examples.
//!
//! A [`PromptTemplate`] bundles a `{{variable}}`-substituted body with optional
//! few-shot example strings rendered before the body. Substitution reuses
//! [`crate::llm::prompt::render_prompt`], so missing variables are left as-is.

use serde::{Deserialize, Serialize};

/// A prompt template with optional few-shot examples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// Template body containing `{{variable}}` placeholders.
    pub template: String,
    /// Few-shot example strings rendered before the template body.
    #[serde(default)]
    pub few_shot: Vec<String>,
}

impl PromptTemplate {
    /// Create a template from a body string.
    pub fn new(template: impl Into<String>) -> Self {
        Self {
            template: template.into(),
            few_shot: Vec::new(),
        }
    }

    /// Set the few-shot examples, returning the updated template.
    pub fn with_few_shot(mut self, examples: Vec<String>) -> Self {
        self.few_shot = examples;
        self
    }

    /// Render the few-shot examples then the substituted template body.
    ///
    /// Each few-shot example is emitted on its own line before the template
    /// body. Variable substitution reuses [`crate::llm::prompt::render_prompt`],
    /// so any `{{variable}}` without a matching entry is left as-is.
    pub fn render(&self, vars: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for ex in &self.few_shot {
            out.push_str(ex);
            out.push('\n');
        }
        out.push_str(&crate::llm::prompt::render_prompt(&self.template, vars));
        out
    }

    /// Extract the `{{variable}}` names referenced by the template, in first-seen order.
    ///
    /// Scans the template body for tokens between the literal strings `"{{"`
    /// and `"}}"`, returning the unique names in order of first appearance.
    /// Few-shot examples are not scanned.
    pub fn variables(&self) -> Vec<String> {
        let mut seen = Vec::new();
        let body = self.template.as_str();
        let mut rest = body;
        while let Some(start) = rest.find("{{") {
            rest = &rest[start + "{{".len()..];
            let Some(end) = rest.find("}}") else { break };
            let name = &rest[..end];
            if !seen.iter().any(|s: &String| s == name) {
                seen.push(name.to_string());
            }
            rest = &rest[end + "}}".len()..];
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_variables() {
        let t = PromptTemplate::new("Hello {{name}}, your topic is {{topic}}.");
        let out = t.render(&[("name", "Alice"), ("topic", "Rust")]);
        assert_eq!(out, "Hello Alice, your topic is Rust.");
    }

    #[test]
    fn few_shot_renders_before_body() {
        let t = PromptTemplate::new("Classify: {{text}}").with_few_shot(vec![
            "Q: apples\nA: fruit".to_string(),
            "Q: rover\nA: dog".to_string(),
        ]);
        let out = t.render(&[("text", "carrot")]);
        let body_pos = out.find("Classify:").expect("body present");
        let ex1_pos = out.find("Q: apples").expect("example 1 present");
        let ex2_pos = out.find("Q: rover").expect("example 2 present");
        assert!(ex1_pos < body_pos, "first example must precede body");
        assert!(ex2_pos < body_pos, "second example must precede body");
        assert!(out.ends_with("Classify: carrot"));
    }

    #[test]
    fn serde_roundtrip_equal() {
        let t = PromptTemplate::new("Summarize: {{input}}")
            .with_few_shot(vec!["Example one".to_string(), "Example two".to_string()]);
        let json = serde_json::to_string(&t).expect("serialize");
        let back: PromptTemplate = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.template, t.template);
        assert_eq!(back.few_shot, t.few_shot);
        assert!(!back.few_shot.is_empty());
    }

    #[test]
    fn missing_variable_left_as_is() {
        let t = PromptTemplate::new("Hello {{name}}, age {{age}}.");
        let out = t.render(&[("name", "Bob")]);
        assert_eq!(out, "Hello Bob, age {{age}}.");
    }

    #[test]
    fn variables_in_first_seen_order() {
        let t = PromptTemplate::new("{{b}} {{a}} {{b}} {{c}}");
        assert_eq!(t.variables(), vec!["b", "a", "c"]);
    }
}
