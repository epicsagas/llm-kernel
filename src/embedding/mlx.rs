//! Rust-native MLX embedding provider on Apple Silicon.
//!
//! Backs [`EmbeddingProvider`] with Apple's MLX array framework via the
//! `mlx-rs` crate (oxideai/mlx-rs — **not** the `mlxrs` crate). MLX runs on
//! the Apple Silicon GPU through unified memory, so this is the throughput
//! path that complements candle-Metal (`embedding-metal`, which wins on
//! single-embed latency).
//!
//! Initial target: `BAAI/bge-small-en-v1.5` (12-layer BERT encoder, CLS
//! pooling, L2-normalised, 384-dim). The encoder forward pass is assembled
//! from `mlx-rs` `nn` modules and the `fast::scaled_dot_product_attention`
//! kernel — `nn::MultiHeadAttention` is deliberately avoided because its
//! parameter names don't line up with the BERT checkpoint's
//! `attention.self.{query,key,value,output}.dense` tensors.
//!
//! ```ignore
//! use llm_kernel::embedding::MlxEmbeddingProvider;
//! use llm_kernel::embedding::EmbeddingProvider;
//!
//! let provider = MlxEmbeddingProvider::new()?;
//! let result = provider.embed("hello world")?;
//! assert_eq!(result.vector.len(), 384);
//! ```

use std::sync::Mutex;

use crate::embedding::types::{EmbeddingProvider, EmbeddingResult};
use crate::error::{KernelError, Result};

/// Default HuggingFace repo for bge-small-en-v1.5.
pub const BGE_SMALL_EN_V15: &str = "BAAI/bge-small-en-v1.5";

/// bge-small-en-v1.5 architecture constants (verified from config.json).
const HIDDEN: usize = 384;
const NUM_HEADS: usize = 12;
const NUM_LAYERS: usize = 12;
#[allow(dead_code)] // documents the architecture; weights carry the dims
const INTERMEDIATE: usize = 1536;
const MAX_POS: usize = 512;
const VOCAB: usize = 30522;
const HEAD_DIM: usize = HIDDEN / NUM_HEADS;

/// Query prefix bge uses for asymmetric retrieval.
const QUERY_PREFIX: &str = "Represent this sentence for searching relevant passages: ";

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

/// Loaded model state: embeddings, per-layer weights, final LayerNorm, tokenizer.
///
/// `Array` is `!Send` (it wraps a C `mlx_array` handle), so this whole struct
/// lives behind a `Mutex`; the `unsafe impl Send` below documents that all
/// access is externally serialised.
struct MlxBge {
    word_embed: mlx_rs::nn::Embedding,
    pos_embed: mlx_rs::nn::Embedding,
    token_type_embed: mlx_rs::nn::Embedding,
    layers: Vec<LayerWeights>,
    final_ln: mlx_rs::nn::LayerNorm,
    tokenizer: tokenizers::Tokenizer,
}

// SAFETY: MLX `Array` holds a C handle the binding declines to mark `Send`.
// All access to `MlxBge` is serialised through the enclosing `Mutex`, so no
// two threads observe an `Array` concurrently. MLX's C library permits handle
// use from any thread under external serialisation.
unsafe impl Send for MlxBge {}

// ---------------------------------------------------------------------------
// Public provider
// ---------------------------------------------------------------------------

/// Embedding provider backed by Rust-native MLX on Apple Silicon.
///
/// Mirrors [`FastembedProvider`](super::FastembedProvider)'s `Mutex` pattern:
/// MLX `Array` is `!Send`, so inference state is serialised behind a lock and
/// the provider stays `Send + Sync` (as `EmbeddingProvider` requires).
pub struct MlxEmbeddingProvider {
    inner: Mutex<MlxBge>,
    model_id: &'static str,
}

impl MlxEmbeddingProvider {
    /// Create a provider for the default model (`bge-small-en-v1.5`).
    ///
    /// Downloads the model from HuggingFace on first call (cached locally).
    /// On Apple Silicon MLX routes inference to the GPU automatically.
    pub fn new() -> Result<Self> {
        Self::with_repo(BGE_SMALL_EN_V15)
    }

    /// Create a provider for an explicit HuggingFace repo.
    pub fn with_repo(repo: &str) -> Result<Self> {
        let model = load_model(repo)?;
        Ok(Self {
            inner: Mutex::new(model),
            model_id: BGE_SMALL_EN_V15,
        })
    }

    /// The HuggingFace model repo ID.
    pub fn model_id(&self) -> &str {
        self.model_id
    }

    /// Tokenise, run the encoder forward pass, CLS-pool, L2-normalise.
    fn run_one(&self, input: &str, preview: &str) -> Result<EmbeddingResult> {
        use crate::embedding::types::{normalize, text_preview};
        use tokenizers::TruncationParams;

        let mut model = self
            .inner
            .lock()
            .map_err(|_| KernelError::Embedding("mlx embedding model mutex poisoned".into()))?;

        let mut tok = model.tokenizer.clone();
        tok.with_truncation(Some(TruncationParams {
            max_length: MAX_POS,
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
        let pooled = encoder_forward(&mut model, &ids_arr, &mask, seq)?;
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
        HIDDEN
    }

    fn name(&self) -> &str {
        self.model_id
    }

    fn embed(&self, text: &str) -> Result<EmbeddingResult> {
        // Query path: bge prepends the instruction prefix.
        self.run_one(&format!("{QUERY_PREFIX}{text}"), text)
    }

    // bge is asymmetric: documents/passages get NO query prefix. Override only
    // this; embed_batch / embed_documents fall back to the trait defaults.
    fn embed_document(&self, text: &str) -> Result<EmbeddingResult> {
        self.run_one(text, text)
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

/// Run the 12-layer BERT encoder and return the CLS-token embedding (raw,
/// pre-normalisation — the caller normalises).
fn encoder_forward(
    model: &mut MlxBge,
    ids: &mlx_rs::Array,
    mask: &[i32],
    seq: usize,
) -> Result<mlx_rs::Array> {
    use mlx_rs::Array;
    use mlx_rs::module::Module;

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

    let scale = 1.0 / (HEAD_DIM as f32).sqrt();

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

        let qh = split_heads(&q, seq)?;
        let kh = split_heads(&k, seq)?;
        let vh = split_heads(&v, seq)?;

        let ctx = mlx_rs::fast::scaled_dot_product_attention(&qh, &kh, &vh, scale, &bias)
            .map_err(|e| KernelError::embedding(format!("sdpa: {e}")))?;

        let ctx = merge_heads(&ctx, seq)?;
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

    let h = model
        .final_ln
        .forward(&h)
        .map_err(|e| KernelError::embedding(format!("final ln: {e}")))?;

    // CLS pooling: row 0 of [1, seq, HIDDEN].
    let flat = h
        .reshape(&[1, (seq * HIDDEN) as i32])
        .map_err(|e| KernelError::embedding(format!("reshape pre-slice: {e}")))?;
    let vals = eval_to_vec_f32(&flat)?;
    Ok(Array::from_slice(&vals[..HIDDEN], &[HIDDEN as i32]))
}

/// Split [1, seq, HIDDEN] -> [1, NUM_HEADS, seq, HEAD_DIM].
fn split_heads(x: &mlx_rs::Array, seq: usize) -> Result<mlx_rs::Array> {
    let reshaped = x
        .reshape(&[1, seq as i32, NUM_HEADS as i32, HEAD_DIM as i32])
        .map_err(|e| KernelError::embedding(format!("split_heads reshape: {e}")))?;
    reshaped
        .transpose_axes(&[0, 2, 1, 3])
        .map_err(|e| KernelError::embedding(format!("split_heads transpose: {e}")))
}

/// Merge [1, NUM_HEADS, seq, HEAD_DIM] -> [1, seq, HIDDEN].
fn merge_heads(x: &mlx_rs::Array, seq: usize) -> Result<mlx_rs::Array> {
    let t = x
        .transpose_axes(&[0, 2, 1, 3])
        .map_err(|e| KernelError::embedding(format!("merge_heads transpose: {e}")))?;
    t.reshape(&[1, seq as i32, HIDDEN as i32])
        .map_err(|e| KernelError::embedding(format!("merge_heads reshape: {e}")))
}

// ---------------------------------------------------------------------------
// Model loading
// ---------------------------------------------------------------------------

fn load_model(repo: &str) -> Result<MlxBge> {
    let api = hf_hub::api::sync::ApiBuilder::new()
        .build()
        .map_err(|e| KernelError::embedding(format!("hf-hub api: {e}")))?;
    let model_repo = api.model(repo.to_string());
    let tok_path = model_repo
        .get("tokenizer.json")
        .map_err(|e| KernelError::embedding(format!("fetch tokenizer.json: {e}")))?;
    let weights_path = model_repo
        .get("model.safetensors")
        .map_err(|e| KernelError::embedding(format!("fetch model.safetensors: {e}")))?;

    let mut tokenizer = tokenizers::Tokenizer::from_file(tok_path)
        .map_err(|e| KernelError::embedding(format!("load tokenizer: {e}")))?;
    tokenizer.with_padding(None);

    let bytes = std::fs::read(&weights_path)
        .map_err(|e| KernelError::embedding(format!("read safetensors: {e}")))?;
    let st = safetensors::SafeTensors::deserialize(&bytes)
        .map_err(|e| KernelError::embedding(format!("parse safetensors: {e}")))?;

    // bge-small checkpoints may prefix tensors with `bert.` or `model.`.
    // Probe the actual keys once and pick the prefix that matches.
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

    // --- Embeddings ---
    let word_embed = embed_from(&w, "embeddings.word_embeddings.weight", VOCAB)?;
    let pos_embed = embed_from(&w, "embeddings.position_embeddings.weight", MAX_POS)?;
    let token_type_embed = embed_from(&w, "embeddings.token_type_embeddings.weight", 2)?;

    // --- Layers ---
    let mut layers = Vec::with_capacity(NUM_LAYERS);
    for i in 0..NUM_LAYERS {
        let a = format!("encoder.layer.{i}.attention");
        let q = linear_from(&w, &format!("{a}.self.query"))?;
        let k = linear_from(&w, &format!("{a}.self.key"))?;
        let v = linear_from(&w, &format!("{a}.self.value"))?;
        let o = linear_from(&w, &format!("{a}.output.dense"))?;
        let attn_ln = layernorm_from(&w, &format!("{a}.output.LayerNorm"))?;
        let fc1 = linear_from(&w, &format!("encoder.layer.{i}.intermediate.dense"))?;
        let fc2 = linear_from(&w, &format!("encoder.layer.{i}.output.dense"))?;
        let ffn_ln = layernorm_from(&w, &format!("encoder.layer.{i}.output.LayerNorm"))?;
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

    // bge's CLS path applies the encoder's per-layer LayerNorms; there is no
    // dedicated post-encoder norm in the safetensors. Use a fresh identity
    // LayerNorm (weight=1, bias=0 from the builder's affine init) as a no-op
    // so the CLS output is passed through unchanged — matching what
    // sentence-transformers' default normalises.
    let final_ln = mlx_rs::nn::LayerNorm {
        dimensions: HIDDEN as i32,
        eps: 1e-12,
        weight: mlx_rs::module::Param::new(Some(mlx_rs::Array::from_slice(
            &vec![1.0f32; HIDDEN],
            &[HIDDEN as i32],
        ))),
        bias: mlx_rs::module::Param::new(Some(mlx_rs::Array::from_slice(
            &vec![0.0f32; HIDDEN],
            &[HIDDEN as i32],
        ))),
    };

    Ok(MlxBge {
        word_embed,
        pos_embed,
        token_type_embed,
        layers,
        final_ln,
        tokenizer,
    })
}

/// Build an `Embedding` and load its weight matrix from `{leaf}`.
fn embed_from(
    w: &dyn Fn(&str) -> Result<mlx_rs::Array>,
    leaf: &str,
    num_embeddings: usize,
) -> Result<mlx_rs::nn::Embedding> {
    use mlx_rs::module::Param;
    use mlx_rs::nn::Embedding;
    let weight = w(leaf)?;
    let _ = num_embeddings;
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
) -> Result<mlx_rs::nn::LayerNorm> {
    use mlx_rs::module::Param;
    use mlx_rs::nn::LayerNorm;
    let weight = w(&format!("{leaf}.weight"))?;
    let bias = w(&format!("{leaf}.bias"))?;
    Ok(LayerNorm {
        dimensions: HIDDEN as i32,
        eps: 1e-12,
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
    fn model_id_constant() {
        assert_eq!(BGE_SMALL_EN_V15, "BAAI/bge-small-en-v1.5");
    }

    #[test]
    fn dims_are_bge_small() {
        assert_eq!(HIDDEN, 384);
        assert_eq!(NUM_HEADS, 12);
        assert_eq!(NUM_LAYERS, 12);
        assert_eq!(HEAD_DIM, 32);
    }

    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn embed_returns_384_dim_normalised() {
        let p = MlxEmbeddingProvider::new().unwrap();
        let r = p.embed("hello world").unwrap();
        assert_eq!(r.vector.len(), 384);
        assert!(!r.vector.is_empty());
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
    fn embed_document_vs_query_asymmetric() {
        let p = MlxEmbeddingProvider::new().unwrap();
        let q = p.embed("capital of France").unwrap();
        let d = p.embed_document("capital of France").unwrap();
        let cos = cosine_similarity(&q.vector, &d.vector);
        assert!(cos > 0.5 && cos < 0.9999, "cos={cos}");
    }

    // Determinism + relatedness ranking — a backend-agnostic regression check
    // that does not require a Python reference. Guards against silent weight
    // mis-loads (e.g. wrong tensor-name prefix) that produce plausible-looking
    // but semantically broken vectors.
    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn embed_is_deterministic_and_ranks_relatedness() {
        let p = MlxEmbeddingProvider::new().unwrap();
        let a = p.embed("hello world").unwrap();
        let b = p.embed("hello world").unwrap();
        let det = cosine_similarity(&a.vector, &b.vector);
        assert!((det - 1.0).abs() < 1e-5, "not deterministic: {det}");

        let q = p.embed("What is the capital of France?").unwrap();
        let related = p.embed_document("The capital of France is Paris.").unwrap();
        let unrelated = p
            .embed_document("Machine learning models process data.")
            .unwrap();
        let rel = cosine_similarity(&q.vector, &related.vector);
        let unrel = cosine_similarity(&q.vector, &unrelated.vector);
        eprintln!("determinism={det:.6} related={rel:.6} unrelated={unrel:.6}");
        assert!(
            rel > unrel,
            "relatedness ranking wrong: related {rel} <= unrelated {unrel}"
        );
    }
}
