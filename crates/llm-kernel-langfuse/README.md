# llm-kernel-langfuse

[Langfuse](https://langfuse.com) observability adapter for
[llm-kernel](https://github.com/epicsagas/llm-kernel). Reports each
non-streaming `complete()` call as a `generation` span — model, token
usage, prompts (optional), and errors — via OpenTelemetry OTLP/JSON to
`{host}/api/public/otel/v1/traces`, the ingestion path Langfuse
recommends for languages without an official SDK (the legacy
`POST /api/public/ingestion` API is deprecated).

llm-kernel itself stays vendor-free; this adapter is the only crate that
knows Langfuse exists.

## Usage

```rust
use llm_kernel::llm::{LLMClient, LLMRequest, MiddlewareClient, OpenAIClient};
use llm_kernel_langfuse::{LangfuseConfig, LangfuseMiddleware};

let config = LangfuseConfig::from_env().expect("LANGFUSE_* keys set");
let middleware = LangfuseMiddleware::new(config);
let client = OpenAIClient::from_key("gpt-4o", "sk-...")?;
let observed = MiddlewareClient::new(client, middleware.clone());
let response = observed
    .complete(LLMRequest::builder().user_message("hello").build())
    .await?;

// On SIGTERM / before exit — flushes the last batch.
// opentelemetry 0.32 has no global shutdown helper; keep the handle.
middleware.shutdown();
```

## Configuration

| Env var | Purpose |
|---------|---------|
| `LANGFUSE_PUBLIC_KEY` | Public key (required for `from_env`) |
| `LANGFUSE_SECRET_KEY` | Secret key (required for `from_env`) |
| `LANGFUSE_HOST` | Self-hosted base URL (default `https://cloud.langfuse.com`) |
| `LANGFUSE_TRACING_ENVIRONMENT` | `langfuse.environment` (e.g. `production`) — set it or local/CI traces pollute production dashboards |
| `LANGFUSE_RELEASE` | `langfuse.release` version tag |

`LangfuseConfig.capture_io = false` drops only the
`langfuse.observation.input`/`.output` span attributes. Model, usage,
level, and metadata are always reported, so cost and failure dashboards
stay alive with the kill switch on.

## Sharing across clients

Build **one** `LangfuseMiddleware` per application and `clone()` it into
every `MiddlewareClient` (fallback chains, per-model clients). Clones
share a single batch exporter and thread; constructing one middleware
per client spawns one exporter thread each.

## Transport notes

- **`x-langfuse-ingestion-version: 4` is always sent** — without it
  Langfuse delays directly-ingested OTel data by up to 10 minutes.
- The exporter uses the **blocking reqwest client** on purpose: the
  OTel batch processor exports from a dedicated thread via `block_on`,
  and an async client panics inside that thread — the service keeps
  running while every trace is silently dropped.
- Spans are queued and exported off-thread in batches; the LLM call
  path never performs network I/O in middleware hooks.

## Scope

- **Streaming is not observed** — `MiddlewareClient::stream_complete`
  does not fire middleware hooks (llm-kernel limitation).
- **Per-call observability** (llm-kernel 0.31+): set
  `LLMRequest::observability` to drive the span name, nest under a
  caller-opened trace (W3C `traceparent`), set the session per call
  (overriding the config default), and attach tags/metadata. The
  middleware's `elapsed` measurement sets real span timing.
- Error `status_message` is masked via
  `llm_kernel::safety::mask_secrets` — provider error bodies can echo
  request credentials.
