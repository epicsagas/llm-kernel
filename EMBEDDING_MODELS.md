# Supported Embedding Models

llm-kernel supports **46 embedding models** across three backends.

## ONNX Models (fastembed-rs)

44 models via ONNX Runtime. Enable with `embedding-fastembed` feature.
No API key required — models download from HuggingFace on first use.

| Model | Dim | Quantized | Description |
|-------|-----|-----------|-------------|
| **BGE / BAAI** | | | |
| `BGESmallENV15` | 384 | | v1.5 release of the fast and default English model |
| `BGESmallENV15Q` | 384 | ✅ | Quantized v1.5 release of the fast and default English model |
| `BGEBaseENV15` | 768 | | v1.5 release of the base English model |
| `BGEBaseENV15Q` | 768 | ✅ | Quantized v1.5 release of the base English model |
| `BGELargeENV15` | 1024 | | v1.5 release of the large English model |
| `BGELargeENV15Q` | 1024 | ✅ | Quantized v1.5 release of the large English model |
| `BGESmallZHV15` | 512 | | v1.5 release of the small Chinese model |
| `BGELargeZHV15` | 1024 | | v1.5 release of the large Chinese model |
| `BGEM3` | 1024 | | Multilingual M3 model with 8192 context length, 100+ languages |
| **Sentence Transformers** | | | |
| `AllMiniLML6V2` | 384 | | Sentence Transformer model, MiniLM-L6-v2 |
| `AllMiniLML6V2Q` | 384 | ✅ | Quantized Sentence Transformer model, MiniLM-L6-v2 |
| `AllMiniLML12V2` | 384 | | Sentence Transformer model, MiniLM-L12-v2 |
| `AllMiniLML12V2Q` | 384 | ✅ | Quantized Sentence Transformer model, MiniLM-L12-v2 |
| `AllMpnetBaseV2` | 768 | | Sentence Transformer model, mpnet-base-v2 |
| **Nomic** | | | |
| `NomicEmbedTextV1` | 768 | | 8192 context length english model |
| `NomicEmbedTextV15` | 768 | | v1.5 release of the 8192 context length english model |
| `NomicEmbedTextV15Q` | 768 | ✅ | Quantized v1.5 release of the 8192 context length english model |
| **Paraphrase** | | | |
| `ParaphraseMLMiniLML12V2` | 384 | | Multi-lingual model |
| `ParaphraseMLMiniLML12V2Q` | 384 | ✅ | Quantized multi-lingual model |
| `ParaphraseMLMpnetBaseV2` | 768 | | Sentence-transformers model for clustering or semantic search |
| **ModernBERT** | | | |
| `ModernBertEmbedLarge` | 1024 | | Large model of ModernBert Text Embeddings |
| **E5 Multilingual** | | | |
| `MultilingualE5Small` | 384 | | Small model of multilingual E5 Text Embeddings |
| `MultilingualE5Base` | 768 | | Base model of multilingual E5 Text Embeddings |
| `MultilingualE5Large` | 1024 | | Large model of multilingual E5 Text Embeddings |
| **Mixedbread** | | | |
| `MxbaiEmbedLargeV1` | 1024 | | Large English embedding model from MixedBreed.ai |
| `MxbaiEmbedLargeV1Q` | 1024 | ✅ | Quantized large English embedding model from MixedBreed.ai |
| **GTE (Alibaba)** | | | |
| `GTEBaseENV15` | 768 | | Base multilingual embedding model from Alibaba |
| `GTEBaseENV15Q` | 768 | ✅ | Quantized base multilingual embedding model from Alibaba |
| `GTELargeENV15` | 1024 | | Large multilingual embedding model from Alibaba |
| `GTELargeENV15Q` | 1024 | ✅ | Quantized large multilingual embedding model from Alibaba |
| **CLIP** | | | |
| `ClipVitB32` | 512 | | CLIP text encoder based on ViT-B/32 (image+text) |
| **Jina** | | | |
| `JinaEmbeddingsV2BaseCode` | 768 | | Jina embeddings v2 base code |
| `JinaEmbeddingsV2BaseEN` | 768 | | Jina embeddings v2 base English |
| **Gemma** | | | |
| `EmbeddingGemma300M` | 768 | | EmbeddingGemma 300M parameter model from Google |
| **Snowflake Arctic** | | | |
| `SnowflakeArcticEmbedXS` | 384 | | Snowflake Arctic embed model, xs |
| `SnowflakeArcticEmbedXSQ` | 384 | ✅ | Quantized Snowflake Arctic embed model, xs |
| `SnowflakeArcticEmbedS` | 384 | | Snowflake Arctic embed model, small |
| `SnowflakeArcticEmbedSQ` | 384 | ✅ | Quantized Snowflake Arctic embed model, small |
| `SnowflakeArcticEmbedM` | 768 | | Snowflake Arctic embed model, medium |
| `SnowflakeArcticEmbedMQ` | 768 | ✅ | Quantized Snowflake Arctic embed model, medium |
| `SnowflakeArcticEmbedMLong` | 768 | | Snowflake Arctic embed model, medium with 2048 context |
| `SnowflakeArcticEmbedMLongQ` | 768 | ✅ | Quantized Snowflake Arctic embed model, medium with 2048 context |
| `SnowflakeArcticEmbedL` | 1024 | | Snowflake Arctic embed model, large |
| `SnowflakeArcticEmbedLQ` | 1024 | ✅ | Quantized Snowflake Arctic embed model, large |

### Query/Document Prefixes

Some models require text prefixes for optimal results:

| Model family | Query prefix | Document prefix |
|-------------|-------------|----------------|
| E5 (`MultilingualE5*`) | `query: ` | `passage: ` |
| Snowflake Arctic | `Represent this sentence for searching relevant passages: ` | _(none)_ |

Prefixes are automatically prepended by `FastembedProvider`.

## Candle Models (pure Rust)

No ONNX Runtime — uses candle-nn for GPU/CPU inference.

### Qwen3

Enable with `embedding-fastembed-qwen3` feature.

| HuggingFace repo | Params | Description |
|-----------------|--------|-------------|
| `Qwen/Qwen3-Embedding-0.6B` | 600M | Lightweight Qwen3 embedding |
| `Qwen/Qwen3-Embedding-8B` | 8B | Full-size Qwen3 embedding |
| `Qwen/Qwen3-VL-Embedding-2B` | 2B | Qwen3 VL embedding (text-only mode) |

```rust
use llm_kernel::embedding::{Qwen3Provider, EmbeddingProvider};

let provider = Qwen3Provider::new("Qwen/Qwen3-Embedding-0.6B")?;
let result = provider.embed("hello world")?;
```

### Nomic V2 MoE

Enable with `embedding-fastembed-nomic-moe` feature.

| HuggingFace repo | Active params | Total params | Dim |
|-----------------|--------------|-------------|-----|
| `nomic-ai/nomic-embed-text-v2-moe` | 305M | 475M | 768 |

8 experts with top-2 routing, hidden_size=768.

```rust
use llm_kernel::embedding::{NomicMoeProvider, EmbeddingProvider};

let provider = NomicMoeProvider::new()?;
let result = provider.embed("hello world")?;
```

## Remote API

| Provider | Feature | Models |
|----------|---------|--------|
| OpenAI | `embedding-openai` | `text-embedding-3-small`, `text-embedding-3-large`, `text-embedding-ada-002` |

```rust
use llm_kernel::embedding::{OpenAIEmbeddingClient, EmbeddingProvider};

let client = OpenAIEmbeddingClient::new("text-embedding-3-small", &api_key)?;
let result = client.embed("hello world")?;
```

## Quick comparison

| Backend | Feature | Offline | Dim range | Notes |
|---------|---------|---------|-----------|-------|
| ONNX (fastembed) | `embedding-fastembed` | ✅ | 384–1024 | 44 models, auto-download |
| Qwen3 (candle) | `embedding-fastembed-qwen3` | ✅ | varies | Pure Rust, GPU support |
| Nomic V2 MoE (candle) | `embedding-fastembed-nomic-moe` | ✅ | 768 | MoE, lightweight |
| OpenAI | `embedding-openai` | ❌ | 1536–3072 | Remote API |
