//! Rust-native MLX embedding provider on Apple Silicon.
//!
//! Backs [`EmbeddingProvider`] with Apple's MLX array framework via the
//! `mlx-rs` crate (oxideai/mlx-rs — **not** the `mlxrs` crate). MLX runs on
//! the Apple Silicon GPU through unified memory, so this is the throughput
//! path that complements candle-Metal (`embedding-metal`, which wins on
//! single-embed latency).
//!
//! Supports any vanilla-BERT encoder model in the catalog whose
//! [`EmbeddingModel::mlx_supported()`] returns `true` — 13 base models
//! (21 variants incl. quantized aliases): BGE-en-v1.5 (small/base/large),
//! bge-small-zh-v1.5, all-MiniLM-L6/L12, paraphrase-multilingual-MiniLM,
//! multilingual-e5-small, Snowflake Arctic Embed (xs/s/m/l), mxbai-embed-large.
//!
//! Membership was established by probing each candidate's original weight repo
//! (`config.json` + the `model.safetensors` header) — a model qualifies only if
//! it is `architectures: ["BertModel"]`, `model_type: "bert"`, absolute position
//! embeddings, gelu, and carries the full `encoder.layer.N.*` tensor layout this
//! forward pass indexes. Notable exclusions: `arctic-embed-m-long` and Nomic v1/v1.5
//! are `NomicBertModel` (`encoder.layers.N.attn.Wqkv`), GTE is `NewModel`, E5
//! base/large and paraphrase-mpnet are `XLMRobertaModel`, mpnet is `MPNetForMaskedLM`,
//! and `bge-large-zh-v1.5` ships only `pytorch_model.bin` (no safetensors).
//!
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
    /// `embeddings.LayerNorm` — applied to the summed embeddings before layer 0.
    /// BERT always has this; omitting it silently corrupts every output vector.
    embed_ln: mlx_rs::nn::LayerNorm,
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
        // Truncation is configured once in `load_model` — no per-call clone of the
        // tokenizer (which would deep-copy the whole 30k+ entry vocab every embed).
        let enc = encoder
            .tokenizer
            .encode(input, true)
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

    // Copy the scalars out so `model` stays free for the `&mut` module calls below.
    let hidden = model.cfg.hidden;
    let num_heads = model.cfg.num_heads;
    let pooling = model.cfg.pooling;
    let head_dim = model.cfg.head_dim();

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
    // BERT normalises the summed embeddings before the first encoder layer.
    h = model
        .embed_ln
        .forward(&h)
        .map_err(|e| KernelError::embedding(format!("embeddings ln: {e}")))?;

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

        let qh = split_heads(&q, seq, num_heads, head_dim)?;
        let kh = split_heads(&k, seq, num_heads, head_dim)?;
        let vh = split_heads(&v, seq, num_heads, head_dim)?;

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
    match pooling {
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
    // `dim()` reports the catalog constant while vectors are sized by the loaded
    // config — if the upstream repo ever disagrees, fail here rather than handing
    // a wrong-width vector to a vector index that was created from `dim()`.
    if hf.hidden_size != model.dimension() {
        return Err(KernelError::Embedding(format!(
            "{model:?}: config.json hidden_size {} != catalog dimension {} (repo {repo})",
            hf.hidden_size,
            model.dimension()
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

    // Configure padding + truncation once here; `run_one` then encodes without
    // cloning the tokenizer on every call.
    let mut tokenizer = tokenizers::Tokenizer::from_file(tok_path)
        .map_err(|e| KernelError::embedding(format!("load tokenizer: {e}")))?;
    tokenizer.with_padding(None);
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams {
            max_length: cfg.max_seq,
            ..Default::default()
        }))
        .map_err(|e| KernelError::embedding(format!("tokenizer truncation: {e}")))?;

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
        let data = decode_f32(view.dtype(), view.data(), &full)?;
        let shape: Vec<i32> = view.shape().iter().map(|&d| d as i32).collect();
        let expected: usize = view.shape().iter().product();
        if data.len() != expected {
            return Err(KernelError::Embedding(format!(
                "tensor {full}: decoded {} elements but shape {:?} needs {expected}",
                data.len(),
                view.shape()
            )));
        }
        Ok(mlx_rs::Array::from_slice(&data, &shape))
    };

    let word_embed = embed_from(&w, "embeddings.word_embeddings.weight")?;
    let pos_embed = embed_from(&w, "embeddings.position_embeddings.weight")?;
    let token_type_embed = embed_from(&w, "embeddings.token_type_embeddings.weight")?;
    let embed_ln = layernorm_from(&w, "embeddings.LayerNorm", cfg.eps)?;

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
        embed_ln,
        layers,
        tokenizer,
    })
}

/// Decode a safetensors byte payload into `f32`, honouring the declared dtype.
///
/// Checkpoints in this family ship F32 (BGE, MiniLM, Arctic) or F16 (mxbai);
/// BF16 is accepted too since upstream repos re-upload in it. Anything else is
/// an explicit error rather than a silent misread — blindly treating bytes as
/// f32 would decode F16 as garbage that still normalises to 1.0.
fn decode_f32(dtype: safetensors::Dtype, raw: &[u8], name: &str) -> Result<Vec<f32>> {
    use safetensors::Dtype;

    let width = match dtype {
        Dtype::F32 => 4,
        Dtype::F16 | Dtype::BF16 => 2,
        other => {
            return Err(KernelError::Embedding(format!(
                "tensor {name}: unsupported dtype {other:?} (expected F32, F16 or BF16)"
            )));
        }
    };
    if !raw.len().is_multiple_of(width) {
        return Err(KernelError::Embedding(format!(
            "tensor {name}: {} bytes is not a multiple of the {width}-byte {dtype:?} element",
            raw.len()
        )));
    }

    Ok(match dtype {
        Dtype::F32 => raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        // IEEE half -> f32 via the standard bit-layout widening.
        Dtype::F16 => raw
            .chunks_exact(2)
            .map(|c| f16_bits_to_f32(u16::from_le_bytes([c[0], c[1]])))
            .collect(),
        // bfloat16 is the top 16 bits of an f32, so widening is a shift.
        Dtype::BF16 => raw
            .chunks_exact(2)
            .map(|c| f32::from_bits((u16::from_le_bytes([c[0], c[1]]) as u32) << 16))
            .collect(),
        _ => unreachable!("dtype filtered above"),
    })
}

/// Widen IEEE-754 binary16 bits to `f32`, preserving subnormals, inf and NaN.
fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) as u32) << 31;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x3ff) as u32;

    match exp {
        // Zero / subnormal. A subnormal half is exactly `mant * 2^-24`, which
        // f32 represents as a normal — let the FPU do the renormalisation
        // instead of hand-rolling the shift.
        0 if mant == 0 => f32::from_bits(sign),
        0 => {
            let mag = mant as f32 * 5.960_464_5e-8; // 2^-24
            if sign != 0 { -mag } else { mag }
        }
        // Inf / NaN.
        0x1f => f32::from_bits(sign | 0x7f80_0000 | (mant << 13)),
        // Normal: rebias the exponent (15 -> 127).
        _ => f32::from_bits(sign | ((exp + 127 - 15) << 23) | (mant << 13)),
    }
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
        // Vanilla-BERT family supported (verified against each weight repo).
        assert!(EmbeddingModel::BGESmallENV15.mlx_supported());
        assert!(EmbeddingModel::AllMiniLML6V2.mlx_supported());
        assert!(EmbeddingModel::SnowflakeArcticEmbedL.mlx_supported());
        assert!(EmbeddingModel::MultilingualE5Small.mlx_supported()); // BertModel
        assert!(EmbeddingModel::MxbaiEmbedLargeV1.mlx_supported()); // BertModel, F16

        // Non-BERT architectures.
        assert!(!EmbeddingModel::BGEM3.mlx_supported()); // XLM-RoBERTa
        assert!(!EmbeddingModel::AllMpnetBaseV2.mlx_supported()); // MPNet
        assert!(!EmbeddingModel::ModernBertEmbedLarge.mlx_supported()); // ModernBERT
        assert!(!EmbeddingModel::GTEBaseENV15.mlx_supported()); // NewModel
        assert!(!EmbeddingModel::JinaEmbeddingsV2BaseEN.mlx_supported()); // JinaBert
        assert!(!EmbeddingModel::NomicEmbedTextV15.mlx_supported()); // NomicBert
        // e5 base/large are XLM-R even though e5-small is BERT.
        assert!(!EmbeddingModel::MultilingualE5Base.mlx_supported());
        assert!(!EmbeddingModel::MultilingualE5Large.mlx_supported());
        // arctic-m-long is NomicBertModel, unlike the rest of the Arctic line.
        assert!(!EmbeddingModel::SnowflakeArcticEmbedMLong.mlx_supported());
        // BertModel, but the repo has no safetensors — only pytorch_model.bin.
        assert!(!EmbeddingModel::BGELargeZHV15.mlx_supported());
    }

    /// Every MLX-supported model must resolve to a repo that actually carries
    /// the original weights — never an ONNX-only `Xenova/*` / `Qdrant/*` mirror.
    #[test]
    fn mlx_repo_never_points_at_onnx_mirror() {
        for &m in EmbeddingModel::ALL {
            if m.mlx_supported() {
                let repo = m.mlx_repo();
                assert!(
                    !repo.starts_with("Xenova/") && !repo.starts_with("Qdrant/"),
                    "{m:?} mlx_repo {repo} is an ONNX mirror with no model.safetensors"
                );
            }
        }
    }

    #[test]
    fn f16_decode_matches_reference_values() {
        // Exact halves: sign, zero, one, and a fraction.
        assert_eq!(f16_bits_to_f32(0x0000), 0.0);
        assert_eq!(f16_bits_to_f32(0x8000), -0.0);
        assert_eq!(f16_bits_to_f32(0x3C00), 1.0);
        assert_eq!(f16_bits_to_f32(0xBC00), -1.0);
        assert_eq!(f16_bits_to_f32(0x4000), 2.0);
        assert_eq!(f16_bits_to_f32(0x3800), 0.5);
        // Largest normal half = 65504.
        assert_eq!(f16_bits_to_f32(0x7BFF), 65504.0);
        // Subnormals: value == mant * 2^-24, both signs.
        assert!((f16_bits_to_f32(0x0001) - 2f32.powi(-24)).abs() < 1e-30);
        assert!((f16_bits_to_f32(0x8001) + 2f32.powi(-24)).abs() < 1e-30);
        // Largest subnormal = 1023 * 2^-24, just below the smallest normal 2^-14.
        assert!((f16_bits_to_f32(0x03FF) - 1023.0 * 2f32.powi(-24)).abs() < 1e-30);
        // Smallest positive normal = 2^-14.
        assert!((f16_bits_to_f32(0x0400) - 2f32.powi(-14)).abs() < 1e-30);
        // Inf / NaN.
        assert!(f16_bits_to_f32(0x7C00).is_infinite());
        assert!(f16_bits_to_f32(0xFC00).is_infinite() && f16_bits_to_f32(0xFC00) < 0.0);
        assert!(f16_bits_to_f32(0x7E00).is_nan());
    }

    #[test]
    fn decode_f32_handles_each_supported_dtype() {
        use safetensors::Dtype;
        // f32 1.0 little-endian.
        assert_eq!(
            decode_f32(Dtype::F32, &1.0f32.to_le_bytes(), "t").unwrap(),
            vec![1.0]
        );
        // f16 1.0 = 0x3C00.
        assert_eq!(
            decode_f32(Dtype::F16, &0x3C00u16.to_le_bytes(), "t").unwrap(),
            vec![1.0]
        );
        // bf16 1.0 = top 16 bits of f32 1.0 (0x3F80).
        assert_eq!(
            decode_f32(Dtype::BF16, &0x3F80u16.to_le_bytes(), "t").unwrap(),
            vec![1.0]
        );
        // Unsupported dtype is a hard error, not a silent misread.
        assert!(decode_f32(Dtype::I64, &[0u8; 8], "t").is_err());
        // Truncated payload is rejected rather than silently dropped.
        assert!(decode_f32(Dtype::F32, &[0u8; 6], "t").is_err());
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

    /// Numerical ground truth vs HuggingFace `transformers`.
    ///
    /// Relatedness ranking and L2 norm both pass even with a structurally wrong
    /// encoder (a missing `embeddings.LayerNorm` still yields deterministic,
    /// unit-norm, correctly-ranked vectors), so those checks cannot detect a
    /// broken forward pass. Only an element-wise comparison against the
    /// reference implementation can. Values produced by:
    ///
    /// ```python
    /// h = AutoModel.from_pretrained(repo)(**tok(text, return_tensors="pt")).last_hidden_state
    /// v = h[:, 0]                       # CLS  (mean+mask for MiniLM)
    /// v = torch.nn.functional.normalize(v, p=2, dim=1)[0][:8]
    /// ```
    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn matches_transformers_reference_vectors() {
        // BGE-small-en-v1.5, CLS pooling, query prefix applied by `embed`.
        let bge = MlxEmbeddingProvider::new(EmbeddingModel::BGESmallENV15).unwrap();
        let got = bge.embed("hello world").unwrap().vector;
        let want = [
            -0.027733, -0.028816, -0.00888, -0.040712, 0.041487, -0.020241, -0.009069, 0.051351,
        ];
        for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() < 2e-3,
                "bge dim {i}: got {g}, want {w} (delta {})",
                (g - w).abs()
            );
        }

        // all-MiniLM-L6-v2, mask-weighted mean pooling, no prefix.
        let mini = MlxEmbeddingProvider::new(EmbeddingModel::AllMiniLML6V2).unwrap();
        let got = mini.embed("hello world").unwrap().vector;
        let want = [
            -0.034477, 0.031023, 0.006735, 0.026109, -0.039362, -0.160303, 0.066924, -0.006441,
        ];
        for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() < 2e-3,
                "minilm dim {i}: got {g}, want {w} (delta {})",
                (g - w).abs()
            );
        }
    }

    /// Smoke-load every newly-admitted model family so a wrong `mlx_supported`
    /// entry surfaces as a load failure rather than at a user's first embed.
    /// mxbai exercises the F16 decode path; e5-small the mean-pool + `query:`
    /// prefix path; arctic the CLS path at 1024 dims.
    #[test]
    #[ignore = "requires model download + Apple Silicon (macOS)"]
    fn newly_supported_models_load_and_embed() {
        for m in [
            EmbeddingModel::MxbaiEmbedLargeV1,
            EmbeddingModel::MultilingualE5Small,
            EmbeddingModel::SnowflakeArcticEmbedXS,
        ] {
            let p = MlxEmbeddingProvider::new(m)
                .unwrap_or_else(|e| panic!("{m:?} failed to load: {e}"));
            let r = p.embed("hello world").unwrap();
            assert_eq!(r.vector.len(), m.dimension(), "{m:?} wrong dim");
            let norm: f64 = r
                .vector
                .iter()
                .map(|x| (*x as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            assert!((norm - 1.0).abs() < 1e-4, "{m:?} L2 norm {norm} != 1.0");
            assert!(
                r.vector.iter().any(|x| *x != 0.0) && r.vector.iter().all(|x| x.is_finite()),
                "{m:?} produced a degenerate vector"
            );
        }
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
