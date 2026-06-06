//! Zero-dep embedding model catalog.
//!
//! Mirrors `fastembed::EmbeddingModel` (44 variants) so the catalog is always
//! available — even when the `embedding-fastembed` feature is disabled.

/// Embedding model catalog with metadata for all supported ONNX models.
///
/// Variant names match `fastembed::EmbeddingModel` exactly for trivial 1:1 mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EmbeddingModel {
    // ── sentence-transformers ───────────────────────
    #[default]
    BGESmallENV15,
    AllMiniLML6V2,
    AllMiniLML6V2Q,
    AllMiniLML12V2,
    AllMiniLML12V2Q,
    AllMpnetBaseV2,
    // ── BGE family ──────────────────────────────────
    BGEBaseENV15,
    BGEBaseENV15Q,
    BGELargeENV15,
    BGELargeENV15Q,
    BGESmallENV15Q,
    BGESmallZHV15,
    BGELargeZHV15,
    BGEM3,
    // ── Nomic ───────────────────────────────────────
    NomicEmbedTextV1,
    NomicEmbedTextV15,
    NomicEmbedTextV15Q,
    // ── Paraphrase ──────────────────────────────────
    ParaphraseMLMiniLML12V2,
    ParaphraseMLMiniLML12V2Q,
    ParaphraseMLMpnetBaseV2,
    // ── ModernBERT ──────────────────────────────────
    ModernBertEmbedLarge,
    // ── E5 multilingual ─────────────────────────────
    MultilingualE5Small,
    MultilingualE5Base,
    MultilingualE5Large,
    // ── Mixedbread ──────────────────────────────────
    MxbaiEmbedLargeV1,
    MxbaiEmbedLargeV1Q,
    // ── GTE (Alibaba) ──────────────────────────────
    GTEBaseENV15,
    GTEBaseENV15Q,
    GTELargeENV15,
    GTELargeENV15Q,
    // ── CLIP ────────────────────────────────────────
    ClipVitB32,
    // ── Jina ────────────────────────────────────────
    JinaEmbeddingsV2BaseCode,
    JinaEmbeddingsV2BaseEN,
    // ── Gemma ───────────────────────────────────────
    EmbeddingGemma300M,
    // ── Snowflake Arctic ────────────────────────────
    SnowflakeArcticEmbedXS,
    SnowflakeArcticEmbedXSQ,
    SnowflakeArcticEmbedS,
    SnowflakeArcticEmbedSQ,
    SnowflakeArcticEmbedM,
    SnowflakeArcticEmbedMQ,
    SnowflakeArcticEmbedMLong,
    SnowflakeArcticEmbedMLongQ,
    SnowflakeArcticEmbedL,
    SnowflakeArcticEmbedLQ,
}

impl EmbeddingModel {
    /// Embedding dimensionality.
    pub const fn dimension(self) -> usize {
        match self {
            // 384-dim
            Self::BGESmallENV15
            | Self::AllMiniLML6V2
            | Self::AllMiniLML6V2Q
            | Self::AllMiniLML12V2
            | Self::AllMiniLML12V2Q
            | Self::BGESmallENV15Q
            | Self::ParaphraseMLMiniLML12V2
            | Self::ParaphraseMLMiniLML12V2Q
            | Self::MultilingualE5Small
            | Self::SnowflakeArcticEmbedXS
            | Self::SnowflakeArcticEmbedXSQ
            | Self::SnowflakeArcticEmbedS
            | Self::SnowflakeArcticEmbedSQ => 384,
            // 512-dim
            Self::BGESmallZHV15 | Self::ClipVitB32 => 512,
            // 768-dim
            Self::AllMpnetBaseV2
            | Self::BGEBaseENV15
            | Self::BGEBaseENV15Q
            | Self::NomicEmbedTextV1
            | Self::NomicEmbedTextV15
            | Self::NomicEmbedTextV15Q
            | Self::ParaphraseMLMpnetBaseV2
            | Self::MultilingualE5Base
            | Self::GTEBaseENV15
            | Self::GTEBaseENV15Q
            | Self::JinaEmbeddingsV2BaseCode
            | Self::JinaEmbeddingsV2BaseEN
            | Self::EmbeddingGemma300M
            | Self::SnowflakeArcticEmbedM
            | Self::SnowflakeArcticEmbedMQ
            | Self::SnowflakeArcticEmbedMLong
            | Self::SnowflakeArcticEmbedMLongQ => 768,
            // 1024-dim
            Self::BGELargeENV15
            | Self::BGELargeENV15Q
            | Self::BGELargeZHV15
            | Self::BGEM3
            | Self::ModernBertEmbedLarge
            | Self::MultilingualE5Large
            | Self::MxbaiEmbedLargeV1
            | Self::MxbaiEmbedLargeV1Q
            | Self::GTELargeENV15
            | Self::GTELargeENV15Q
            | Self::SnowflakeArcticEmbedL
            | Self::SnowflakeArcticEmbedLQ => 1024,
        }
    }

    /// Short human-readable description.
    pub const fn description(self) -> &'static str {
        match self {
            Self::BGESmallENV15 => "v1.5 release of the fast and default English model",
            Self::AllMiniLML6V2 => "Sentence Transformer model, MiniLM-L6-v2",
            Self::AllMiniLML6V2Q => "Quantized Sentence Transformer model, MiniLM-L6-v2",
            Self::AllMiniLML12V2 => "Sentence Transformer model, MiniLM-L12-v2",
            Self::AllMiniLML12V2Q => "Quantized Sentence Transformer model, MiniLM-L12-v2",
            Self::AllMpnetBaseV2 => "Sentence Transformer model, mpnet-base-v2",
            Self::BGEBaseENV15 => "v1.5 release of the base English model",
            Self::BGEBaseENV15Q => "Quantized v1.5 release of the base English model",
            Self::BGELargeENV15 => "v1.5 release of the large English model",
            Self::BGELargeENV15Q => "Quantized v1.5 release of the large English model",
            Self::BGESmallENV15Q => "Quantized v1.5 release of the fast and default English model",
            Self::NomicEmbedTextV1 => "8192 context length english model",
            Self::NomicEmbedTextV15 => "v1.5 release of the 8192 context length english model",
            Self::NomicEmbedTextV15Q => {
                "Quantized v1.5 release of the 8192 context length english model"
            }
            Self::ParaphraseMLMiniLML12V2 => "Multi-lingual model",
            Self::ParaphraseMLMiniLML12V2Q => "Quantized multi-lingual model",
            Self::ParaphraseMLMpnetBaseV2 => {
                "Sentence-transformers model for clustering or semantic search"
            }
            Self::BGESmallZHV15 => "v1.5 release of the small Chinese model",
            Self::BGELargeZHV15 => "v1.5 release of the large Chinese model",
            Self::BGEM3 => "Multilingual M3 model with 8192 context length, 100+ languages",
            Self::ModernBertEmbedLarge => "Large model of ModernBert Text Embeddings",
            Self::MultilingualE5Small => "Small model of multilingual E5 Text Embeddings",
            Self::MultilingualE5Base => "Base model of multilingual E5 Text Embeddings",
            Self::MultilingualE5Large => "Large model of multilingual E5 Text Embeddings",
            Self::MxbaiEmbedLargeV1 => "Large English embedding model from MixedBreed.ai",
            Self::MxbaiEmbedLargeV1Q => {
                "Quantized large English embedding model from MixedBreed.ai"
            }
            Self::GTEBaseENV15 => "Base multilingual embedding model from Alibaba",
            Self::GTEBaseENV15Q => "Quantized base multilingual embedding model from Alibaba",
            Self::GTELargeENV15 => "Large multilingual embedding model from Alibaba",
            Self::GTELargeENV15Q => "Quantized large multilingual embedding model from Alibaba",
            Self::ClipVitB32 => "CLIP text encoder based on ViT-B/32",
            Self::JinaEmbeddingsV2BaseCode => "Jina embeddings v2 base code",
            Self::JinaEmbeddingsV2BaseEN => "Jina embeddings v2 base English",
            Self::EmbeddingGemma300M => "EmbeddingGemma 300M parameter model from Google",
            Self::SnowflakeArcticEmbedXS => "Snowflake Arctic embed model, xs",
            Self::SnowflakeArcticEmbedXSQ => "Quantized Snowflake Arctic embed model, xs",
            Self::SnowflakeArcticEmbedS => "Snowflake Arctic embed model, small",
            Self::SnowflakeArcticEmbedSQ => "Quantized Snowflake Arctic embed model, small",
            Self::SnowflakeArcticEmbedM => "Snowflake Arctic embed model, medium",
            Self::SnowflakeArcticEmbedMQ => "Quantized Snowflake Arctic embed model, medium",
            Self::SnowflakeArcticEmbedMLong => {
                "Snowflake Arctic embed model, medium with 2048 context"
            }
            Self::SnowflakeArcticEmbedMLongQ => {
                "Quantized Snowflake Arctic embed model, medium with 2048 context"
            }
            Self::SnowflakeArcticEmbedL => "Snowflake Arctic embed model, large",
            Self::SnowflakeArcticEmbedLQ => "Quantized Snowflake Arctic embed model, large",
        }
    }

    /// Optional prefix prepended to query texts before embedding.
    pub const fn query_prefix(self) -> Option<&'static str> {
        match self {
            Self::MultilingualE5Small | Self::MultilingualE5Base | Self::MultilingualE5Large => {
                Some("query: ")
            }
            Self::SnowflakeArcticEmbedXS
            | Self::SnowflakeArcticEmbedXSQ
            | Self::SnowflakeArcticEmbedS
            | Self::SnowflakeArcticEmbedSQ
            | Self::SnowflakeArcticEmbedM
            | Self::SnowflakeArcticEmbedMQ
            | Self::SnowflakeArcticEmbedMLong
            | Self::SnowflakeArcticEmbedMLongQ
            | Self::SnowflakeArcticEmbedL
            | Self::SnowflakeArcticEmbedLQ => {
                Some("Represent this sentence for searching relevant passages: ")
            }
            _ => None,
        }
    }

    /// Optional prefix prepended to document texts before embedding.
    pub const fn doc_prefix(self) -> Option<&'static str> {
        match self {
            Self::MultilingualE5Small | Self::MultilingualE5Base | Self::MultilingualE5Large => {
                Some("passage: ")
            }
            _ => None,
        }
    }

    /// Approximate ONNX model size in MB.
    pub const fn size_mb(self) -> usize {
        match self {
            // 40 MB
            Self::BGESmallENV15 | Self::BGESmallENV15Q => 40,
            // 80 MB
            Self::AllMiniLML6V2 | Self::AllMiniLML6V2Q => 80,
            // 90 MB
            Self::SnowflakeArcticEmbedXS | Self::SnowflakeArcticEmbedXSQ | Self::BGESmallZHV15 => {
                90
            }
            // 120 MB
            Self::AllMiniLML12V2 | Self::AllMiniLML12V2Q => 120,
            // 130 MB
            Self::SnowflakeArcticEmbedS | Self::SnowflakeArcticEmbedSQ => 130,
            // 260 MB
            Self::JinaEmbeddingsV2BaseCode
            | Self::JinaEmbeddingsV2BaseEN
            | Self::EmbeddingGemma300M => 260,
            // 420 MB
            Self::AllMpnetBaseV2
            | Self::BGEBaseENV15
            | Self::BGEBaseENV15Q
            | Self::GTEBaseENV15
            | Self::GTEBaseENV15Q => 420,
            // 430 MB
            Self::SnowflakeArcticEmbedM
            | Self::SnowflakeArcticEmbedMQ
            | Self::SnowflakeArcticEmbedMLong
            | Self::SnowflakeArcticEmbedMLongQ => 430,
            // 470 MB
            Self::ParaphraseMLMiniLML12V2
            | Self::ParaphraseMLMiniLML12V2Q
            | Self::MultilingualE5Small => 470,
            // 550 MB
            Self::NomicEmbedTextV1 | Self::NomicEmbedTextV15 | Self::NomicEmbedTextV15Q => 550,
            // 600 MB
            Self::BGEM3 | Self::ModernBertEmbedLarge | Self::ClipVitB32 => 600,
            // 970 MB
            Self::ParaphraseMLMpnetBaseV2 | Self::MultilingualE5Base => 970,
            // 1300 MB
            Self::BGELargeENV15
            | Self::BGELargeENV15Q
            | Self::BGELargeZHV15
            | Self::MultilingualE5Large
            | Self::MxbaiEmbedLargeV1
            | Self::MxbaiEmbedLargeV1Q
            | Self::GTELargeENV15
            | Self::GTELargeENV15Q
            | Self::SnowflakeArcticEmbedL
            | Self::SnowflakeArcticEmbedLQ => 1300,
        }
    }

    /// HuggingFace repository ID (e.g. `"BAAI/bge-small-en-v1.5"`).
    pub const fn model_id(self) -> &'static str {
        match self {
            Self::BGESmallENV15 => "BAAI/bge-small-en-v1.5",
            Self::AllMiniLML6V2 => "sentence-transformers/all-MiniLM-L6-v2",
            Self::AllMiniLML6V2Q => "Xenova/all-MiniLM-L6-v2",
            Self::AllMiniLML12V2 => "sentence-transformers/all-MiniLM-L12-v2",
            Self::AllMiniLML12V2Q => "Xenova/all-MiniLM-L12-v2",
            Self::AllMpnetBaseV2 => "sentence-transformers/all-mpnet-base-v2",
            Self::BGEBaseENV15 => "BAAI/bge-base-en-v1.5",
            Self::BGEBaseENV15Q => "Qdrant/bge-base-en-v1.5-onnx-Q",
            Self::BGELargeENV15 => "BAAI/bge-large-en-v1.5",
            Self::BGELargeENV15Q => "Qdrant/bge-large-en-v1.5-onnx-Q",
            Self::BGESmallENV15Q => "Qdrant/bge-small-en-v1.5-onnx-Q",
            Self::NomicEmbedTextV1 => "nomic-ai/nomic-embed-text-v1",
            Self::NomicEmbedTextV15 => "nomic-ai/nomic-embed-text-v1.5",
            Self::NomicEmbedTextV15Q => "nomic-ai/nomic-embed-text-v1.5",
            Self::ParaphraseMLMiniLML12V2 => {
                "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
            }
            Self::ParaphraseMLMiniLML12V2Q => "Xenova/paraphrase-multilingual-MiniLM-L12-v2",
            Self::ParaphraseMLMpnetBaseV2 => {
                "sentence-transformers/paraphrase-multilingual-mpnet-base-v2"
            }
            Self::BGESmallZHV15 => "BAAI/bge-small-zh-v1.5",
            Self::BGELargeZHV15 => "BAAI/bge-large-zh-v1.5",
            Self::BGEM3 => "BAAI/bge-m3",
            Self::ModernBertEmbedLarge => "nomic-ai/modernbert-embed-large",
            Self::MultilingualE5Small => "intfloat/multilingual-e5-small",
            Self::MultilingualE5Base => "intfloat/multilingual-e5-base",
            Self::MultilingualE5Large => "intfloat/multilingual-e5-large",
            Self::MxbaiEmbedLargeV1 => "mixedbread-ai/mxbai-embed-large-v1",
            Self::MxbaiEmbedLargeV1Q => "mixedbread-ai/mxbai-embed-large-v1",
            Self::GTEBaseENV15 => "Alibaba-NLP/gte-base-en-v1.5",
            Self::GTEBaseENV15Q => "Qdrant/gte-base-en-v1.5-onnx-Q",
            Self::GTELargeENV15 => "Alibaba-NLP/gte-large-en-v1.5",
            Self::GTELargeENV15Q => "Qdrant/gte-large-en-v1.5-onnx-Q",
            Self::ClipVitB32 => "openai/clip-vit-base-patch32",
            Self::JinaEmbeddingsV2BaseCode => "jinaai/jina-embeddings-v2-base-code",
            Self::JinaEmbeddingsV2BaseEN => "jinaai/jina-embeddings-v2-base-en",
            Self::EmbeddingGemma300M => "google/embedding-gemma-300M",
            Self::SnowflakeArcticEmbedXS => "Snowflake/snowflake-arctic-embed-xs",
            Self::SnowflakeArcticEmbedXSQ => "Snowflake/snowflake-arctic-embed-xs",
            Self::SnowflakeArcticEmbedS => "Snowflake/snowflake-arctic-embed-s",
            Self::SnowflakeArcticEmbedSQ => "Snowflake/snowflake-arctic-embed-s",
            Self::SnowflakeArcticEmbedM => "Snowflake/snowflake-arctic-embed-m",
            Self::SnowflakeArcticEmbedMQ => "Snowflake/snowflake-arctic-embed-m",
            Self::SnowflakeArcticEmbedMLong => "Snowflake/snowflake-arctic-embed-m-long",
            Self::SnowflakeArcticEmbedMLongQ => "Snowflake/snowflake-arctic-embed-m-long",
            Self::SnowflakeArcticEmbedL => "Snowflake/snowflake-arctic-embed-l",
            Self::SnowflakeArcticEmbedLQ => "Snowflake/snowflake-arctic-embed-l",
        }
    }

    /// Maximum token context per model.
    pub const fn max_seq_length(self) -> usize {
        match self {
            // 256 tokens
            Self::AllMiniLML6V2
            | Self::AllMiniLML6V2Q
            | Self::AllMiniLML12V2
            | Self::AllMiniLML12V2Q => 256,
            // 384 tokens
            Self::AllMpnetBaseV2 => 384,
            // 8192 tokens
            Self::BGEM3
            | Self::NomicEmbedTextV1
            | Self::NomicEmbedTextV15
            | Self::NomicEmbedTextV15Q
            | Self::JinaEmbeddingsV2BaseCode
            | Self::JinaEmbeddingsV2BaseEN
            | Self::EmbeddingGemma300M
            | Self::SnowflakeArcticEmbedMLong
            | Self::SnowflakeArcticEmbedMLongQ => 8192,
            // 512 tokens (default)
            Self::BGESmallENV15
            | Self::BGESmallENV15Q
            | Self::BGEBaseENV15
            | Self::BGEBaseENV15Q
            | Self::BGELargeENV15
            | Self::BGELargeENV15Q
            | Self::BGESmallZHV15
            | Self::BGELargeZHV15
            | Self::ParaphraseMLMiniLML12V2
            | Self::ParaphraseMLMiniLML12V2Q
            | Self::ParaphraseMLMpnetBaseV2
            | Self::ModernBertEmbedLarge
            | Self::MultilingualE5Small
            | Self::MultilingualE5Base
            | Self::MultilingualE5Large
            | Self::MxbaiEmbedLargeV1
            | Self::MxbaiEmbedLargeV1Q
            | Self::GTEBaseENV15
            | Self::GTEBaseENV15Q
            | Self::GTELargeENV15
            | Self::GTELargeENV15Q
            | Self::ClipVitB32
            | Self::SnowflakeArcticEmbedXS
            | Self::SnowflakeArcticEmbedXSQ
            | Self::SnowflakeArcticEmbedS
            | Self::SnowflakeArcticEmbedSQ
            | Self::SnowflakeArcticEmbedM
            | Self::SnowflakeArcticEmbedMQ
            | Self::SnowflakeArcticEmbedL
            | Self::SnowflakeArcticEmbedLQ => 512,
        }
    }

    /// Whether this is a quantized model (Q suffix).
    pub const fn is_quantized(self) -> bool {
        matches!(
            self,
            Self::AllMiniLML6V2Q
                | Self::AllMiniLML12V2Q
                | Self::BGEBaseENV15Q
                | Self::BGELargeENV15Q
                | Self::BGESmallENV15Q
                | Self::NomicEmbedTextV15Q
                | Self::ParaphraseMLMiniLML12V2Q
                | Self::MxbaiEmbedLargeV1Q
                | Self::GTEBaseENV15Q
                | Self::GTELargeENV15Q
                | Self::SnowflakeArcticEmbedXSQ
                | Self::SnowflakeArcticEmbedSQ
                | Self::SnowflakeArcticEmbedMQ
                | Self::SnowflakeArcticEmbedMLongQ
                | Self::SnowflakeArcticEmbedLQ
        )
    }

    /// Whether this model handles image inputs (CLIP).
    pub const fn is_image_model(self) -> bool {
        matches!(self, Self::ClipVitB32)
    }

    /// String representation matching the enum variant name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BGESmallENV15 => "BGESmallENV15",
            Self::AllMiniLML6V2 => "AllMiniLML6V2",
            Self::AllMiniLML6V2Q => "AllMiniLML6V2Q",
            Self::AllMiniLML12V2 => "AllMiniLML12V2",
            Self::AllMiniLML12V2Q => "AllMiniLML12V2Q",
            Self::AllMpnetBaseV2 => "AllMpnetBaseV2",
            Self::BGEBaseENV15 => "BGEBaseENV15",
            Self::BGEBaseENV15Q => "BGEBaseENV15Q",
            Self::BGELargeENV15 => "BGELargeENV15",
            Self::BGELargeENV15Q => "BGELargeENV15Q",
            Self::BGESmallENV15Q => "BGESmallENV15Q",
            Self::NomicEmbedTextV1 => "NomicEmbedTextV1",
            Self::NomicEmbedTextV15 => "NomicEmbedTextV15",
            Self::NomicEmbedTextV15Q => "NomicEmbedTextV15Q",
            Self::ParaphraseMLMiniLML12V2 => "ParaphraseMLMiniLML12V2",
            Self::ParaphraseMLMiniLML12V2Q => "ParaphraseMLMiniLML12V2Q",
            Self::ParaphraseMLMpnetBaseV2 => "ParaphraseMLMpnetBaseV2",
            Self::BGESmallZHV15 => "BGESmallZHV15",
            Self::BGELargeZHV15 => "BGELargeZHV15",
            Self::BGEM3 => "BGEM3",
            Self::ModernBertEmbedLarge => "ModernBertEmbedLarge",
            Self::MultilingualE5Small => "MultilingualE5Small",
            Self::MultilingualE5Base => "MultilingualE5Base",
            Self::MultilingualE5Large => "MultilingualE5Large",
            Self::MxbaiEmbedLargeV1 => "MxbaiEmbedLargeV1",
            Self::MxbaiEmbedLargeV1Q => "MxbaiEmbedLargeV1Q",
            Self::GTEBaseENV15 => "GTEBaseENV15",
            Self::GTEBaseENV15Q => "GTEBaseENV15Q",
            Self::GTELargeENV15 => "GTELargeENV15",
            Self::GTELargeENV15Q => "GTELargeENV15Q",
            Self::ClipVitB32 => "ClipVitB32",
            Self::JinaEmbeddingsV2BaseCode => "JinaEmbeddingsV2BaseCode",
            Self::JinaEmbeddingsV2BaseEN => "JinaEmbeddingsV2BaseEN",
            Self::EmbeddingGemma300M => "EmbeddingGemma300M",
            Self::SnowflakeArcticEmbedXS => "SnowflakeArcticEmbedXS",
            Self::SnowflakeArcticEmbedXSQ => "SnowflakeArcticEmbedXSQ",
            Self::SnowflakeArcticEmbedS => "SnowflakeArcticEmbedS",
            Self::SnowflakeArcticEmbedSQ => "SnowflakeArcticEmbedSQ",
            Self::SnowflakeArcticEmbedM => "SnowflakeArcticEmbedM",
            Self::SnowflakeArcticEmbedMQ => "SnowflakeArcticEmbedMQ",
            Self::SnowflakeArcticEmbedMLong => "SnowflakeArcticEmbedMLong",
            Self::SnowflakeArcticEmbedMLongQ => "SnowflakeArcticEmbedMLongQ",
            Self::SnowflakeArcticEmbedL => "SnowflakeArcticEmbedL",
            Self::SnowflakeArcticEmbedLQ => "SnowflakeArcticEmbedLQ",
        }
    }

    /// Parse a model name (case-insensitive).
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .find(|m| m.as_str().eq_ignore_ascii_case(s))
            .copied()
            .ok_or_else(|| format!("unknown embedding model: {s}"))
    }

    /// All supported models.
    pub const ALL: &[Self] = &[
        Self::BGESmallENV15,
        Self::AllMiniLML6V2,
        Self::AllMiniLML6V2Q,
        Self::AllMiniLML12V2,
        Self::AllMiniLML12V2Q,
        Self::AllMpnetBaseV2,
        Self::BGEBaseENV15,
        Self::BGEBaseENV15Q,
        Self::BGELargeENV15,
        Self::BGELargeENV15Q,
        Self::BGESmallENV15Q,
        Self::NomicEmbedTextV1,
        Self::NomicEmbedTextV15,
        Self::NomicEmbedTextV15Q,
        Self::ParaphraseMLMiniLML12V2,
        Self::ParaphraseMLMiniLML12V2Q,
        Self::ParaphraseMLMpnetBaseV2,
        Self::BGESmallZHV15,
        Self::BGELargeZHV15,
        Self::BGEM3,
        Self::ModernBertEmbedLarge,
        Self::MultilingualE5Small,
        Self::MultilingualE5Base,
        Self::MultilingualE5Large,
        Self::MxbaiEmbedLargeV1,
        Self::MxbaiEmbedLargeV1Q,
        Self::GTEBaseENV15,
        Self::GTEBaseENV15Q,
        Self::GTELargeENV15,
        Self::GTELargeENV15Q,
        Self::ClipVitB32,
        Self::JinaEmbeddingsV2BaseCode,
        Self::JinaEmbeddingsV2BaseEN,
        Self::EmbeddingGemma300M,
        Self::SnowflakeArcticEmbedXS,
        Self::SnowflakeArcticEmbedXSQ,
        Self::SnowflakeArcticEmbedS,
        Self::SnowflakeArcticEmbedSQ,
        Self::SnowflakeArcticEmbedM,
        Self::SnowflakeArcticEmbedMQ,
        Self::SnowflakeArcticEmbedMLong,
        Self::SnowflakeArcticEmbedMLongQ,
        Self::SnowflakeArcticEmbedL,
        Self::SnowflakeArcticEmbedLQ,
    ];

    /// Map to `fastembed::EmbeddingModel`.
    ///
    /// Only available when the `embedding-fastembed` feature is enabled.
    #[cfg(feature = "embedding-fastembed")]
    pub fn as_fastembed(self) -> fastembed::EmbeddingModel {
        match self {
            Self::BGESmallENV15 => fastembed::EmbeddingModel::BGESmallENV15,
            Self::AllMiniLML6V2 => fastembed::EmbeddingModel::AllMiniLML6V2,
            Self::AllMiniLML6V2Q => fastembed::EmbeddingModel::AllMiniLML6V2Q,
            Self::AllMiniLML12V2 => fastembed::EmbeddingModel::AllMiniLML12V2,
            Self::AllMiniLML12V2Q => fastembed::EmbeddingModel::AllMiniLML12V2Q,
            Self::AllMpnetBaseV2 => fastembed::EmbeddingModel::AllMpnetBaseV2,
            Self::BGEBaseENV15 => fastembed::EmbeddingModel::BGEBaseENV15,
            Self::BGEBaseENV15Q => fastembed::EmbeddingModel::BGEBaseENV15Q,
            Self::BGELargeENV15 => fastembed::EmbeddingModel::BGELargeENV15,
            Self::BGELargeENV15Q => fastembed::EmbeddingModel::BGELargeENV15Q,
            Self::BGESmallENV15Q => fastembed::EmbeddingModel::BGESmallENV15Q,
            Self::NomicEmbedTextV1 => fastembed::EmbeddingModel::NomicEmbedTextV1,
            Self::NomicEmbedTextV15 => fastembed::EmbeddingModel::NomicEmbedTextV15,
            Self::NomicEmbedTextV15Q => fastembed::EmbeddingModel::NomicEmbedTextV15Q,
            Self::ParaphraseMLMiniLML12V2 => fastembed::EmbeddingModel::ParaphraseMLMiniLML12V2,
            Self::ParaphraseMLMiniLML12V2Q => fastembed::EmbeddingModel::ParaphraseMLMiniLML12V2Q,
            Self::ParaphraseMLMpnetBaseV2 => fastembed::EmbeddingModel::ParaphraseMLMpnetBaseV2,
            Self::BGESmallZHV15 => fastembed::EmbeddingModel::BGESmallZHV15,
            Self::BGELargeZHV15 => fastembed::EmbeddingModel::BGELargeZHV15,
            Self::BGEM3 => fastembed::EmbeddingModel::BGEM3,
            Self::ModernBertEmbedLarge => fastembed::EmbeddingModel::ModernBertEmbedLarge,
            Self::MultilingualE5Small => fastembed::EmbeddingModel::MultilingualE5Small,
            Self::MultilingualE5Base => fastembed::EmbeddingModel::MultilingualE5Base,
            Self::MultilingualE5Large => fastembed::EmbeddingModel::MultilingualE5Large,
            Self::MxbaiEmbedLargeV1 => fastembed::EmbeddingModel::MxbaiEmbedLargeV1,
            Self::MxbaiEmbedLargeV1Q => fastembed::EmbeddingModel::MxbaiEmbedLargeV1Q,
            Self::GTEBaseENV15 => fastembed::EmbeddingModel::GTEBaseENV15,
            Self::GTEBaseENV15Q => fastembed::EmbeddingModel::GTEBaseENV15Q,
            Self::GTELargeENV15 => fastembed::EmbeddingModel::GTELargeENV15,
            Self::GTELargeENV15Q => fastembed::EmbeddingModel::GTELargeENV15Q,
            Self::ClipVitB32 => fastembed::EmbeddingModel::ClipVitB32,
            Self::JinaEmbeddingsV2BaseCode => fastembed::EmbeddingModel::JinaEmbeddingsV2BaseCode,
            Self::JinaEmbeddingsV2BaseEN => fastembed::EmbeddingModel::JinaEmbeddingsV2BaseEN,
            Self::EmbeddingGemma300M => fastembed::EmbeddingModel::EmbeddingGemma300M,
            Self::SnowflakeArcticEmbedXS => fastembed::EmbeddingModel::SnowflakeArcticEmbedXS,
            Self::SnowflakeArcticEmbedXSQ => fastembed::EmbeddingModel::SnowflakeArcticEmbedXSQ,
            Self::SnowflakeArcticEmbedS => fastembed::EmbeddingModel::SnowflakeArcticEmbedS,
            Self::SnowflakeArcticEmbedSQ => fastembed::EmbeddingModel::SnowflakeArcticEmbedSQ,
            Self::SnowflakeArcticEmbedM => fastembed::EmbeddingModel::SnowflakeArcticEmbedM,
            Self::SnowflakeArcticEmbedMQ => fastembed::EmbeddingModel::SnowflakeArcticEmbedMQ,
            Self::SnowflakeArcticEmbedMLong => fastembed::EmbeddingModel::SnowflakeArcticEmbedMLong,
            Self::SnowflakeArcticEmbedMLongQ => {
                fastembed::EmbeddingModel::SnowflakeArcticEmbedMLongQ
            }
            Self::SnowflakeArcticEmbedL => fastembed::EmbeddingModel::SnowflakeArcticEmbedL,
            Self::SnowflakeArcticEmbedLQ => fastembed::EmbeddingModel::SnowflakeArcticEmbedLQ,
        }
    }
}

impl std::fmt::Display for EmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EmbeddingModel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_count() {
        assert_eq!(EmbeddingModel::ALL.len(), 44);
    }

    #[test]
    fn default_is_bge_small() {
        assert_eq!(EmbeddingModel::default(), EmbeddingModel::BGESmallENV15);
    }

    #[test]
    fn dimension_consistency() {
        for &m in EmbeddingModel::ALL {
            let dim = m.dimension();
            assert!(
                [384, 512, 768, 1024].contains(&dim),
                "{m:?}: unexpected dimension {dim}"
            );
        }
    }

    #[test]
    fn parse_roundtrip() {
        for &m in EmbeddingModel::ALL {
            let s = m.as_str();
            assert_eq!(EmbeddingModel::parse(s).unwrap(), m);
        }
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(
            EmbeddingModel::parse("bgesmallenv15").unwrap(),
            EmbeddingModel::BGESmallENV15
        );
        assert_eq!(
            EmbeddingModel::parse("ALLMINILML6V2").unwrap(),
            EmbeddingModel::AllMiniLML6V2
        );
    }

    #[test]
    fn parse_unknown_fails() {
        assert!(EmbeddingModel::parse("NotARealModel").is_err());
    }

    #[test]
    fn quantized_flags() {
        let quantized: Vec<_> = EmbeddingModel::ALL
            .iter()
            .filter(|m| m.is_quantized())
            .copied()
            .collect();
        // All Q-suffix variants should be flagged
        for m in &quantized {
            assert!(m.as_str().ends_with('Q'), "{m:?} flagged but no Q suffix");
        }
        // Non-Q variants should NOT be flagged
        for &m in EmbeddingModel::ALL {
            if !m.as_str().ends_with('Q') {
                assert!(!m.is_quantized(), "{m:?} not Q but flagged quantized");
            }
        }
    }

    #[test]
    fn image_model_flag() {
        assert!(EmbeddingModel::ClipVitB32.is_image_model());
        assert_eq!(
            EmbeddingModel::ALL
                .iter()
                .filter(|m| m.is_image_model())
                .count(),
            1
        );
    }

    #[test]
    fn prefix_mapping() {
        // E5 models have query + doc prefixes
        for &m in &[
            EmbeddingModel::MultilingualE5Small,
            EmbeddingModel::MultilingualE5Base,
            EmbeddingModel::MultilingualE5Large,
        ] {
            assert_eq!(m.query_prefix(), Some("query: "));
            assert_eq!(m.doc_prefix(), Some("passage: "));
        }
        // Snowflake models have query prefix only
        for &m in &[
            EmbeddingModel::SnowflakeArcticEmbedXS,
            EmbeddingModel::SnowflakeArcticEmbedLQ,
        ] {
            assert!(m.query_prefix().is_some());
            assert!(m.doc_prefix().is_none());
        }
        // Most models have no prefixes
        assert!(EmbeddingModel::BGESmallENV15.query_prefix().is_none());
        assert!(EmbeddingModel::BGESmallENV15.doc_prefix().is_none());
    }

    #[test]
    fn from_str_trait() {
        let m: EmbeddingModel = "BGESmallENV15".parse().unwrap();
        assert_eq!(m, EmbeddingModel::BGESmallENV15);
    }

    #[test]
    fn display_trait() {
        assert_eq!(EmbeddingModel::BGESmallENV15.to_string(), "BGESmallENV15");
    }

    #[test]
    fn metadata_nonzero() {
        for &m in EmbeddingModel::ALL {
            assert!(m.size_mb() > 0, "{m:?}: size_mb is zero");
            assert!(!m.model_id().is_empty(), "{m:?}: model_id is empty");
            assert!(m.max_seq_length() > 0, "{m:?}: max_seq_length is zero");
        }
    }

    #[test]
    fn max_seq_length_values() {
        assert_eq!(EmbeddingModel::AllMiniLML6V2.max_seq_length(), 256);
        assert_eq!(EmbeddingModel::AllMpnetBaseV2.max_seq_length(), 384);
        assert_eq!(EmbeddingModel::BGEM3.max_seq_length(), 8192);
        assert_eq!(EmbeddingModel::BGESmallENV15.max_seq_length(), 512);
    }
}
