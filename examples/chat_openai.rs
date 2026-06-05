//! Basic OpenAI chat completion example.
//!
//! Requires: `cargo run --example chat_openai --features client-async`
//! Set `OPENAI_API_KEY` environment variable before running.

#[cfg(feature = "client-async")]
#[tokio::main]
async fn main() {
    use llm_kernel::llm::{ChatMessage, LLMClient, ModelConfig, OpenAIClient};

    // Resolve model config from catalog or create directly
    let config = ModelConfig {
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key_env: "OPENAI_API_KEY".into(),
        base_url: None,
        temperature: 0.7,
        max_tokens: Some(256),
    };

    let client = OpenAIClient::new(&config).expect("failed to create client");

    let response = client
        .complete(llm_kernel::llm::LLMRequest {
            system: Some("You are a helpful assistant. Reply in one sentence.".into()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "What is llm-kernel?".into(),
            }],
            model: None,
            temperature: 0.7,
            max_tokens: Some(256),
        })
        .await
        .expect("completion failed");

    println!("Model:  {}", response.model);
    println!(
        "Tokens: {} in / {} out",
        response.usage.prompt_tokens, response.usage.completion_tokens
    );
    println!("Reply:  {}", response.content);
}

#[cfg(not(feature = "client-async"))]
fn main() {
    eprintln!("This example requires the client-async feature.");
    eprintln!("Run: cargo run --example chat_openai --features client-async");
}
