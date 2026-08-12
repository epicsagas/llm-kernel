//! Rust-native MLX embedding provider on Apple Silicon.
//!
//! Backs [`EmbeddingProvider`] with Apple's MLX array framework via the
//! `mlx-rs` crate (oxideai/mlx-rs — **not** the `mlxrs` crate). MLX runs on
//! the Apple Silicon GPU through unified memory, so this is the throughput
//! path that complements candle-Metal (`embedding-metal`, which wins on
//! single-embed latency).
//!
//! Supports any vanilla-BERT encoder model in the catalog whose
//! [`EmbeddingModel::mlx_supported()`] returns `true` — currently 14 base
//! architectures (28 variants incl. quantized): BGE-en/zh-v1.5, all-MiniLM-L6/L12,
//! paraphrase-multilingual-MiniLM, Snowflake Arctic Embed (XS–L), mxbai-embed-large.
//! The encoder forward pass is assembled from `mlx-rs` `nn` modules and the
//! `fast::scaled_dot_product_attention` kernel; shape/pooling/prefix come from
//! the catalog + each model's `config.json`.
//!
//! ```ignore
//! use llm_kernel::embedding::{EmbeddingModel, MlxEmbeddingProvider};
//! use llm_kernel::embedding::EmbeddingProvider;
//!
//! let provider = MlxEmbeddingProvider::new(EmbeddingModel::BGESmallENV15)?;
//! let result = provider.embed("hello world")?;
//! ```

use std::sync::Mutex;

use crate::embedding::catalog::EmbeddingModel;
use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};
use crate::error::{KernelError, Result};

// ---------------------------------------------------------------------------
// Config (parsed from each model's config.json at load time)
// ---------------------------------------------------------------------------

/// Pooling strategy applied after the encoder.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pooling {
    /// Take the first token's hidden state (BGE, Arctic, mxbai).
    Cls,
    /// Mask-weighted mean over token positions (MiniLM, paraphrase-ML-MiniLM).
    Mean,
}

/// Per-model BERT architecture parameters, read from `config.json`.
struct BertConfig {
    hidden: usize,
    num_heads: usize,
    num_layers: usize,
    #[allow(dead_code)] // parsed for validation; weights carry the shape
    intermediate: usize,
    #[allow(dead_code)] // parsed for validation; weights carry the shape
    vocab: usize,
    /// Truncation length — the catalog `max_seq_length()`, which for some
    /// models (MiniLM) is shorter than the config's `max_position_embeddings`.
    max_seq: usize,
    pooling: Pooling,
    eps: f32,
}

impl BertConfig {
    fn head_dim(&self) -> usize {
        self.hidden / self.num_heads
    }
}

/// Subset of HuggingFace `config.json` fields we read via serde_json.
#[derive(serde::Deserialize)]
struct HfConfig {
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    #[serde(default = "default_vocab")]
    vocab_size: usize,
    #[serde(default = "default_max_pos")]
    max_position_embeddings: usize,
    #[serde(default = "default_eps")]
    layer_norm_eps: f32,
}

fn default_vocab() -> usize {
    30522
}
fn default_max_pos() -> usize {
    512
}
fn default_eps() -> f32 {
    1e-12
}

// ---------------------------------------------------------------------------
// Weight model
// ---------------------------------------------------------------------------

/// One BERT encoder layer's weights, loaded from the checkpoint.
struct LayerWeights {
    q: mlx_rs::nn::Linear,
    k: mlx_rs::nn::Linear,
    v: mlx_rs::nn::Linear,
    o: mlx_rs::nn::Linear,
    attn_ln: mlx_rs::nn::LayerNorm,
    fc1: mlx_rs::nn::Linear,
    fc2: mlx_rs::nn::Linear,
    ffn_ln: mlx_rs::nn::LayerNorm,
}

/// Loaded model state: config, embeddings, per-layer weights, tokenizer.
///
/// `Array` is `!Send` (it wraps a C `mlx_array` handle), so this whole struct
/// lives behind a `Mutex`; the `unsafe impl Send` below documents that all
/// access is externally serialised.
struct MlxBertEncoder {
    cfg: BertConfig,
    word_embed: mlx_rs::nn::Embedding,
    pos_embed: mlx_rs::nn::Embedding,
    token_type_embed: mlx_rs::nn::Embedding,
    layers: Vec<LayerWeights>,
    tokenizer: tokenizers::Tokenizer,
}

// SAFETY: MLX `Array` holds a C handle the binding declines to mark `Send`.
// All access to `MlxBertEncoder` is serialised through the enclosing `Mutex`,
// so no two threads observe an `Array` concurrently. MLX's C library permits
// handle use from any thread under external serialisation.
unsafe impl Send for MlxBertEncoder {}

// ---------------------------------------------------------------------------
// Public provider
// ---------------------------------------------------------------------------

/// Embedding provider backed by Rust-native MLX on Apple Silicon.
///
/// Mirrors [`FastembedProvider`](super::FastembedProvider)'s `Mutex` pattern:
/// MLX `Array` is `!Send`, so inference state is serialised behind a lock and
/// the provider stays `Send + Sync` (as `EmbeddingProvider` requires).
///
/// Construction takes a catalog [`EmbeddingModel`] — only variants where
/// `mlx_supported()` returns `true` are accepted; others error immediately.
pub struct MlxEmbeddingProvider {
    inner: Mutex<MlxBertEncoder>,
    model: EmbeddingModel,
}

impl MlxEmbeddingProvider {
    /// Create a provider for a catalog model.
    ///
    /// Downloads the model from HuggingFace on first call (cached locally).
    /// On Apple Silicon MLX routes inference to the GPU automatically.
    /// Returns an error if `model.mlx_supported()` is false (non-BERT
    /// architectures are not yet ported to the MLX forward pass).
    pub fn new(model: EmbeddingModel) -> Result<Self> {
        if !model.mlx_supported() {
            return Err(KernelError::Embedding(format!(
                "{model:?} is not a vanilla-BERT architecture; MLX backend not yet ported"
            )));
        }
        let encoder = load_model(model)?;
        Ok(Self {
            inner: Mutex::new(encoder),
            model,
        })
    }

    /// Tokenise, run the encoder forward pass, pool, L2-normalise.
    fn run_one(&self, input: &str, preview: &str) -> Result<EmbeddingResult> {
        use crate::embedding::types::{normalize, text_preview};

        let mut encoder = self
            .inner
            .lock()
            .map_err(|_| KernelError::Embedding("mlx embedding model mutex poisoned".into()))?;
        let max_seq = encoder.cfg.max_seq;

        let mut tok = encoder.tokenizer.clone();
        tok.with_truncation(Some(tokenizers::TruncationParams {
            max_length: max_seq,
            ..Default::default()
        }))
        .map_err(|e| KernelError::embedding(format!("tokenizer truncation: {e}")))?;
        let enc = tok
            .encode(vec![input.to_string()], true)
            .map_err(|e| KernelError::embedding(format!("tokenizer encode: {e}")))?;
        let ids: Vec<i32> = enc.get_ids().iter().map(|&u| u as i32).collect();
        let mask: Vec<i32> = enc.get_attention_mask().iter().map(|&u| u as i32).collect();
        let seq = ids.len();

        let ids_arr = mlx_rs::Array::from_slice(&ids, &[1, seq as i32]);
        let pooled = encoder_forward(&mut encoder, &ids_arr, &mask, seq)?;
        let mut vector = eval_to_vec_f32(&pooled)?;
        normalize(&mut vector);

        Ok(EmbeddingResult {
            vector,
            text_preview: text_preview(preview),
        })
    }
}

impl EmbeddingProvider for MlxEmbeddingProvider {
    fn dim(&self) -> usize {
        self.model.dimension()
    }

    fn name(&self) -> &str {
        self.model.model_code()
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        match self.model.query_prefix() {
            Some(p) => self.run_one(&format!("{p}{text}"), text),
            None => self.run_one(text, text),
        }
    }

    // Asymmetric models (BGE-en, Arctic, mxbai, E5, Nomic-v1.5) prepend a
    // passage prefix to documents; symmetric ones (MiniLM, paraphrase-ML) use
    // the bare text. Catalog encodes both.
    fn embed_document(&self, text: &str) -> Result<EmbeddingResult> {
        match self.model.doc_prefix() {
            Some(p) => self.run_one(&format!("{p}{text}"), text),
            None => self.run_one(text, text),
        }
    }
}

// ---------------------------------------------------------------------------
// Forward pass
// ---------------------------------------------------------------------------

/// Evaluate `arr` and materialise it as a flat `Vec<f32>`.
///
/// MLX is lazy — nothing runs until `eval()`. After eval, the contiguous
/// backing store is read out in row-major order.
fn eval_to_vec_f32(arr: &mlx_rs::Array) -> Result<Vec<f32>> {
    arr.eval()
        .map_err(|e| KernelError::embedding(format!("mlx eval: {e}")))?;
    Ok(arr.as_slice::<f32>().to_vec())
}

/// Run the BERT encoder and return the pooled embedding (raw, pre-normalisation
/// — the caller normalises). Pooling follows `cfg.pooling`.
fn encoder_forward(
    model: &mut MlxBertEncoder,
    ids: &mlx_rs::Array,
    mask: &[i32],
    seq: usize,
) -> Result<mlx_rs::Array> {
    use mlx_rs::Array;
    use mlx_rs::module::Module;

    let cfg = &model.cfg;
    let hidden = cfg.hidden;
    let head_dim = cfg.head_dim();

    // Embeddings: word + position + token_type (token_type=0 throughout).
    let pos_ids = Array::arange::<_, i32>(0, seq as i32, None)
        .map_err(|e| KernelError::embedding(format!("arange: {e}")))?;
    let h = model
        .word_embed
        .forward(ids)
        .map_err(|e| KernelError::embedding(format!("word embed: {e}")))?;
    let pos = model
        .pos_embed
        .forward(&pos_ids)
        .map_err(|e| KernelError::embedding(format!("pos embed: {e}")))?;
    let tt = model
        .token_type_embed
        .forward(&Array::from_slice(&vec![0i32; seq], &[1, seq as i32]))
        .map_err(|e| KernelError::embedding(format!("token_type embed: {e}")))?;
    let mut h = h
        .add(&pos)
        .map_err(|e| KernelError::embedding(format!("add pos: {e}")))?;
    h = h
        .add(&tt)
        .map_err(|e| KernelError::embedding(format!("add token_type: {e}")))?;

    // Attention mask as additive bias: keep=0, pad=-inf, shaped [1,1,1,seq].
    let bias: Vec<f32> = mask
        .iter()
        .map(|&m| if m > 0 { 0.0 } else { f32::NEG_INFINITY })
        .collect();
    let bias = Array::from_slice(&bias, &[1, 1, 1, seq as i32]);

    let scale = 1.0 / (head_dim as f32).sqrt();

    for layer in &mut model.layers {
        // --- Self-attention (post-norm BERT) ---
        let q = layer
            .q
            .forward(&h)
            .map_err(|e| KernelError::embedding(format!("q proj: {e}")))?;
        let k = layer
            .k
            .forward(&h)
            .map_err(|e| KernelError::embedding(format!("k proj: {e}")))?;
        let v = layer
            .v
            .forward(&h)
            .map_err(|e| KernelError::embedding(format!("v proj: {e}")))?;

        let qh = split_heads(&q, seq, cfg.num_heads, head_dim)?;
        let kh = split_heads(&k, seq, cfg.num_heads, head_dim)?;
        let vh = split_heads(&v, seq, cfg.num_heads, head_dim)?;

        let ctx = mlx_rs::fast::scaled_dot_product_attention(&qh, &kh, &vh, scale, &bias)
            .map_err(|e| KernelError::embedding(format!("sdpa: {e}")))?;

        let ctx = merge_heads(&ctx, seq, hidden)?;
        let attn = layer
            .o
            .forward(&ctx)
            .map_err(|e| KernelError::embedding(format!("o proj: {e}")))?;
        let res = h
            .add(&attn)
            .map_err(|e| KernelError::embedding(format!("attn residual: {e}")))?;
        h = layer
            .attn_ln
            .forward(&res)
            .map_err(|e| KernelError::embedding(format!("attn ln: {e}")))?;

        // --- Feed-forward (post-norm) ---
        let ff_in = layer
            .fc1
            .forward(&h)
            .map_err(|e| KernelError::embedding(format!("fc1: {e}")))?;
        let ff =
            mlx_rs::nn::gelu(&ff_in).map_err(|e| KernelError::embedding(format!("gelu: {e}")))?;
        let ff = layer
            .fc2
            .forward(&ff)
            .map_err(|e| KernelError::embedding(format!("fc2: {e}")))?;
        let res = h
            .add(&ff)
            .map_err(|e| KernelError::embedding(format!("ffn residual: {e}")))?;
        h = layer
            .ffn_ln
            .forward(&res)
            .map_err(|e| KernelError::embedding(format!("ffn ln: {e}")))?;
    }

    // Pooling. BERT-family checkpoints have no top-level post-encoder norm —
    // the final layer's `output.LayerNorm` is the last normalisation, so we
    // pool `h` directly (no identity final-LN hack needed).
    match cfg.pooling {
        Pooling::Cls => {
            // Row 0 of [1, seq, hidden].
            let flat = h
                .reshape(&[1, (seq * hidden) as i32])
                .map_err(|e| KernelError::embedding(format!("cls reshape: {e}")))?;
            let vals = eval_to_vec_f32(&flat)?;
            Ok(Array::from_slice(&vals[..hidden], &[hidden as i32]))
        }
        Pooling::Mean => {
            // Mask-weighted mean over the seq axis: [1, seq, hidden] -> [1, hidden].
            let all = eval_to_vec_f32(&h)?;
            let mut summed = vec![0.0f32; hidden];
            let mut count = 0usize;
            for (i, &m) in mask.iter().enumerate() {
                if m > 0 {
                    let row = &all[i * hidden..(i + 1) * hidden];
                    for (s, &x) in summed.iter_mut().zip(row) {
                        *s += x;
                    }
                    count += 1;
                }
            }
            let mean = if count > 0 {
                summed.iter().map(|&x| x / count as f32).collect::<Vec<_>>()
            } else {
                summed
            };
            Ok(Array::from_slice(&mean, &[hidden as i32]))
        }
    }
}

/// Split [1, seq, hidden] -> [1, num_heads, seq, head_dim].
fn split_heads(
    x: &mlx_rs::Array,
    seq: usize,
    num_heads: usize,
    head_dim: usize,
) -> Result<mlx_rs::Array> {
    let reshaped = x
        .reshape(&[1, seq as i32, num_heads as i32, head_dim as i32])
        .map_err(|e| KernelError::embedding(format!("split_heads reshape: {e}")))?;
    reshaped
        .transpose_axes(&[0, 2, 1, 3])
        .map_err(|e| KernelError::embedding(format!("split_heads transpose: {e}")))
}

/// Merge [1, num_heads, seq, head_dim] -> [1, seq, hidden].
fn merge_heads(x: &mlx_rs::Array, seq: usize, hidden: usize) -> Result<mlx_rs::Array> {
    let t = x
        .transpose_axes(&[0, 2, 1, 3])
        .map_err(|e| KernelError::embedding(format!("merge_heads transpose: {e}")))?;
    t.reshape(&[1, seq as i32, hidden as i32])
        .map_err(|e| KernelError::embedding(format!("merge_heads reshape: {e}")))
}

// ---------------------------------------------------------------------------
// Model loading
// ---------------------------------------------------------------------------

fn load_model(model: EmbeddingModel) -> Result<MlxBertEncoder> {
    let repo = model.mlx_repo();
    let api = hf_hub::api::sync::ApiBuilder::new()
        .build()
        .map_err(|e| KernelError::embedding(format!("hf-hub api: {e}")))?;
    let model_repo = api.model(repo.to_string());
    let cfg_path = model_repo
        .get("config.json")
        .map_err(|e| KernelError::embedding(format!("fetch config.json: {e}")))?;
    let tok_path = model_repo
        .get("tokenizer.json")
        .map_err(|e| KernelError::embedding(format!("fetch tokenizer.json: {e}")))?;
    let weights_path = model_repo
        .get("model.safetensors")
        .map_err(|e| KernelError::embedding(format!("fetch model.safetensors: {e}")))?;

    // Parse config.json for the architecture shape constants.
    let cfg_bytes = std::fs::read(&cfg_path)
        .map_err(|e| KernelError::embedding(format!("read config.json: {e}")))?;
    let hf: HfConfig = serde_json::from_slice(&cfg_bytes)
        .map_err(|e| KernelError::embedding(format!("parse config.json: {e}")))?;
    if hf.num_attention_heads == 0 || !hf.hidden_size.is_multiple_of(hf.num_attention_heads) {
        return Err(KernelError::Embedding(format!(
            "bad config: hidden {} not divisible by heads {}",
            hf.hidden_size, hf.num_attention_heads
        )));
    }
    let cfg = BertConfig {
        hidden: hf.hidden_size,
        num_heads: hf.num_attention_heads,
        num_layers: hf.num_hidden_layers,
        intermediate: hf.intermediate_size,
        vocab: hf.vocab_size,
        max_seq: model.max_seq_length().min(hf.max_position_embeddings),
        pooling: if model.uses_cls_pooling() {
            Pooling::Cls
        } else {
            Pooling::Mean
        },
        eps: hf.layer_norm_eps,
    };

    let mut tokenizer = tokenizers::Tokenizer::from_file(tok_path)
        .map_err(|e| KernelError::embedding(format!("load tokenizer: {e}")))?;
    tokenizer.with_padding(None);

    let bytes = std::fs::read(&weights_path)
        .map_err(|e| KernelError::embedding(format!("read safetensors: {e}")))?;
    let st = safetensors::SafeTensors::deserialize(&bytes)
        .map_err(|e| KernelError::embedding(format!("parse safetensors: {e}")))?;

    // Checkpoints may prefix tensors with `bert.` or `model.`. Probe once.
    let names: Vec<String> = st.names().iter().map(|s| s.to_string()).collect();
    let prefix = names
        .iter()
        .find_map(|n| {
            n.strip_suffix("embeddings.word_embeddings.weight")
                .map(|rest| rest.to_string())
        })
        .unwrap_or_default();
    let w = |leaf: &str| -> Result<mlx_rs::Array> {
        let full = format!("{prefix}{leaf}");
        let view = st.tensor(&full).map_err(|_| {
            KernelError::Embedding(format!(
                "missing tensor: {full} (prefix={prefix:?}; sample keys: {:?})",
                &names[..names.len().min(6)]
            ))
        })?;
        let data: Vec<f32> = view
            .data()
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let shape: Vec<i32> = view.shape().iter().map(|&d| d as i32).collect();
        Ok(mlx_rs::Array::from_slice(&data, &shape))
    };

    let word_embed = embed_from(&w, "embeddings.word_embeddings.weight")?;
    let pos_embed = embed_from(&w, "embeddings.position_embeddings.weight")?;
    let token_type_embed = embed_from(&w, "embeddings.token_type_embeddings.weight")?;

    let mut layers = Vec::with_capacity(cfg.num_layers);
    for i in 0..cfg.num_layers {
        let a = format!("encoder.layer.{i}.attention");
        let q = linear_from(&w, &format!("{a}.self.query"))?;
        let k = linear_from(&w, &format!("{a}.self.key"))?;
        let v = linear_from(&w, &format!("{a}.self.value"))?;
        let o = linear_from(&w, &format!("{a}.output.dense"))?;
        let attn_ln = layernorm_from(&w, &format!("{a}.output.LayerNorm"), cfg.eps)?;
        let fc1 = linear_from(&w, &format!("encoder.layer.{i}.intermediate.dense"))?;
        let fc2 = linear_from(&w, &format!("encoder.layer.{i}.output.dense"))?;
        let ffn_ln = layernorm_from(&w, &format!("encoder.layer.{i}.output.LayerNorm"), cfg.eps)?;
        layers.push(LayerWeights {
            q,
            k,
            v,
            o,
            attn_ln,
            fc1,
            fc2,
            ffn_ln,
        });
    }

    Ok(MlxBertEncoder {
        cfg,
        word_embed,
        pos_embed,
        token_type_embed,
        layers,
        tokenizer,
    })
}

/// Build an `Embedding` and load its weight matrix from `{leaf}`.
fn embed_from(
    w: &dyn Fn(&str) -> Result<mlx_rs::Array>,
    leaf: &str,
) -> Result<mlx_rs::nn::Embedding> {
    use mlx_rs::module::Param;
    use mlx_rs::nn::Embedding;
    let weight = w(leaf)?;
    Ok(Embedding {
        weight: Param::new(weight),
    })
}

/// Build a `Linear` and load weight+bias from `{leaf}.weight|.bias`.
fn linear_from(
    w: &dyn Fn(&str) -> Result<mlx_rs::Array>,
    leaf: &str,
) -> Result<mlx_rs::nn::Linear> {
    use mlx_rs::module::Param;
    use mlx_rs::nn::Linear;
    let weight = w(&format!("{leaf}.weight"))?;
    let bias = w(&format!("{leaf}.bias"))?;
    Ok(Linear {
        weight: Param::new(weight),
        bias: Param::new(Some(bias)),
    })
}

/// Build a `LayerNorm` and load weight+bias from `{leaf}.weight|.bias`.
fn layernorm_from(
    w: &dyn Fn(&str) -> Result<mlx_rs::Array>,
    leaf: &str,
    eps: f32,
) -> Result<mlx_rs::nn::LayerNorm> {
    use mlx_rs::module::Param;
    use mlx_rs::nn::LayerNorm;
    let weight = w(&format!("{leaf}.weight"))?;
    let bias = w(&format!("{leaf}.bias"))?;
    let dims = weight.shape()[0];
    Ok(LayerNorm {
        dimensions: dims,
        eps,
        weight: Param::new(Some(weight)),
        bias: Param::new(Some(bias)),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::types::cosine_similarity;

    #[test]
    fn mlx_supported_flag_is_sane() {
        // BERT family supported; representative non-BERT not.
        assert!(EmbeddingModel::BGESmallENV15.mlx_supported());
        assert!(EmbeddingModel::AllMiniLML6V2.mlx_supported());
        assert!(EmbeddingModel::SnowflakeArcticEmbedL.mlx_supported());
        assert!(!EmbeddingModel::BGEM3.mlx_supported()); // XLM-RoBERTa
        assert!(!EmbeddingModel::AllMpnetBaseV2.mlx_supported()); // MPNet
        assert!(!EmbeddingModel::ModernBertEmbedLarge.mlx_supported()); // ModernBERT
    }

    #[test]
    fn pooling_classification() {
        assert!(EmbeddingModel::BGESmallENV15.uses_cls_pooling());
        assert!(EmbeddingModel::MxbaiEmbedLargeV1.uses_cls_pooling());
        assert!(!EmbeddingModel::AllMiniLML6V2.uses_cls_pooling()); // mean
    }

    // Regression: query_prefix() gap fix — BGE-en-v1.5 and mxbai must now
    // return the BGE instruction prefix (previously None).
    #[test]
    fn query_prefix_covers_bge_and_mxbai() {
        assert_eq!(
            EmbeddingModel::BGESmallENV15.query_prefix(),
            Some("Represent this sentence for searching relevant passages: ")
        );
        assert_eq!(
            EmbeddingModel::MxbaiEmbedLargeV1.query_prefix(),
            Some("Represent this sentence for searching relevant passages: ")
        );
        // Symmetric models stay None.
        assert_eq!(EmbeddingModel::AllMiniLML6V2.query_prefix(), None);
    }

    #[test]
    fn rejects_non_bert_model() {
        // ModernBERT is not a vanilla-BERT architecture.
        assert!(MlxEmbeddingProvider::new(EmbeddingModel::ModernBertEmbedLarge).is_err());
    }

    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn bge_small_embeds_384_cls() {
        let p = MlxEmbeddingProvider::new(EmbeddingModel::BGESmallENV15).unwrap();
        let r = p.embed("hello world").unwrap();
        assert_eq!(r.vector.len(), 384);
        let norm: f64 = r
            .vector
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "L2 norm {norm} != 1.0");
    }

    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn minilm_embeds_384_mean() {
        // all-MiniLM-L6-v2: mean pooling, symmetric (no query prefix).
        let p = MlxEmbeddingProvider::new(EmbeddingModel::AllMiniLML6V2).unwrap();
        let r = p.embed("hello world").unwrap();
        assert_eq!(r.vector.len(), 384);
        let norm: f64 = r
            .vector
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "L2 norm {norm} != 1.0");
    }

    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn bge_small_ranks_relatedness() {
        let p = MlxEmbeddingProvider::new(EmbeddingModel::BGESmallENV15).unwrap();
        let q = p.embed("What is the capital of France?").unwrap();
        let related = p.embed_document("The capital of France is Paris.").unwrap();
        let unrelated = p
            .embed_document("Machine learning models process data.")
            .unwrap();
        let rel = cosine_similarity(&q.vector, &related.vector);
        let unrel = cosine_similarity(&q.vector, &unrelated.vector);
        assert!(rel > unrel, "related {rel} <= unrelated {unrel}");
    }

    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn minilm_ranks_relatedness_mean_pool() {
        let p = MlxEmbeddingProvider::new(EmbeddingModel::AllMiniLML6V2).unwrap();
        let q = p.embed("What is the capital of France?").unwrap();
        let related = p.embed_document("The capital of France is Paris.").unwrap();
        let unrelated = p
            .embed_document("Machine learning models process data.")
            .unwrap();
        let rel = cosine_similarity(&q.vector, &related.vector);
        let unrel = cosine_similarity(&q.vector, &unrelated.vector);
        eprintln!("minilm related={rel:.6} unrelated={unrel:.6}");
        assert!(rel > unrel, "related {rel} <= unrelated {unrel}");
    }
}
