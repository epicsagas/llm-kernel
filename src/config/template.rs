/// Returns a default config template for llm-kernel based products.
/// Products should override this with their own template.
pub fn default_config_template(product_name: &str) -> String {
    format!(
        r#"[llm]
provider = "openai"
model = "gpt-4o"
api_key_env = "OPENAI_API_KEY"
temperature = 0.7
max_tokens = 4096

[output]
directory = "./{product_name}-output"

[logging]
level = "info"
"#,
    )
}
