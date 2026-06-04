/// Simple variable substitution in prompt templates.
/// Replaces `{{variable}}` with provided values.
pub fn render_prompt(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{}}}}}", key), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_prompt() {
        let template = "Hello {{name}}, your topic is {{topic}}.";
        let result = render_prompt(template, &[("name", "Alice"), ("topic", "Rust")]);
        assert_eq!(result, "Hello Alice, your topic is Rust.");
    }

    #[test]
    fn test_render_prompt_missing_var() {
        let template = "Hello {{name}}.";
        let result = render_prompt(template, &[]);
        assert_eq!(result, "Hello {{name}}.");
    }
}
