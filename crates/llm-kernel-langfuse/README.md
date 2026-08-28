# llm-kernel-langfuse

[Langfuse](https://langfuse.com) observability adapter for
[llm-kernel](https://github.com/epicsagas/llm-kernel). Ships each
non-streaming `complete()` call to Langfuse as a `generation-create` event —
model, token usage, prompts (optional), and errors.

llm-kernel itself stays vendor-free; this adapter is the only crate that
knows Langfuse exists.

## Usage

```rust
use llm_kernel::llm::{LLMRequest, MiddlewareClient, OpenAIClient};
use llm_kernel_langfuse::{LangfuseConfig, LangfuseMiddleware};

let config = LangfuseConfig::from_env().expect("LANGFUSE_* keys set");
let client = OpenAIClient::from_key("gpt-4o", "sk-...")?;
let observed = MiddlewareClient::new(client, LangfuseMiddleware::new(config));
let response = observed
    .complete(LLMRequest::builder().user_message("hello").build())
    .await?;
```

## Configuration

| Env var | Purpose |
|---------|---------|
| `LANGFUSE_PUBLIC_KEY` | Public key (required for `from_env`) |
| `LANGFUSE_SECRET_KEY` | Secret key (required for `from_env`) |
| `LANGFUSE_HOST` | Self-hosted base URL (default `https://cloud.langfuse.com`) |

`LangfuseConfig.capture_io = false` drops prompts and completions from
events (usage and outcome metadata only) for PII-sensitive deployments.

## Scope

- **Streaming is not observed** — `MiddlewareClient::stream_complete` does
  not fire middleware hooks (llm-kernel limitation).
- One HTTP POST per call, inline from the hook, 5s timeout. Batching can be
  added when volume matters.
- Latency is not reported (would require request-id correlation state).
- Ingestion failures are logged with `tracing::warn!` and never affect the
  LLM call.
