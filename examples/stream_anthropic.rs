//! Anthropic streaming chat example.
//!
//! Requires: `cargo run --example stream_anthropic --features client-async`
//! Set `ANTHROPIC_API_KEY` environment variable before running.

#[cfg(feature = "client-async")]
#[tokio::main]
async fn main() {
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use futures_core::Stream;
    use llm_kernel::llm::{AnthropicClient, ChatMessage, LLMClient, ModelConfig, StreamEvent};

    let config = ModelConfig {
        provider: "anthropic".into(),
        model: "claude-haiku-4-5-20251001".into(),
        api_key_env: "ANTHROPIC_API_KEY".into(),
        base_url: None,
        temperature: 0.7,
        max_tokens: Some(256),
    };

    let client = AnthropicClient::new(&config).expect("failed to create client");

    let stream = client
        .stream_complete(llm_kernel::llm::LLMRequest {
            system: Some("Reply in exactly 3 sentences.".into()),
            messages: vec![ChatMessage::user("What is Rust?")],
            model: None,
            temperature: 0.7,
            max_tokens: Some(256),
            response_format: None,
        })
        .await
        .expect("stream failed");

    let waker = Waker::noop();
    let mut cx = Context::from_waker(&waker);
    let mut stream = stream;

    loop {
        match Pin::new(&mut stream).poll_next(&mut cx) {
            Poll::Ready(Some(Ok(StreamEvent::Delta { content }))) => print!("{}", content),
            Poll::Ready(Some(Ok(StreamEvent::Usage(usage)))) => {
                println!(
                    "\n--- {} in / {} out tokens ---",
                    usage.prompt_tokens, usage.completion_tokens
                );
            }
            Poll::Ready(Some(Ok(StreamEvent::Done))) | Poll::Ready(None) => break,
            Poll::Ready(Some(Err(e))) => {
                eprintln!("\nError: {}", e);
                break;
            }
            Poll::Pending => tokio::task::yield_now().await,
        }
    }
}

#[cfg(not(feature = "client-async"))]
fn main() {
    eprintln!("This example requires the client-async feature.");
    eprintln!("Run: cargo run --example stream_anthropic --features client-async");
}
